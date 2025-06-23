#[macro_use]
extern crate lazy_static;

mod channel;
mod check;
mod clean;
mod client;
mod crypt;
mod error;
mod handler;
mod http;
mod message;
mod metrics;
mod proto;
mod server;
mod state;
mod sync;
mod target;
mod varint;
mod voice;

use crate::clean::clean_loop;
use crate::http::create_http_server;
use crate::proto::mumble::Version;
use crate::server::{create_tcp_server, create_udp_server};
use crate::state::ServerState;
use crate::sync::RwLock;
use clap::Parser;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::TlsAcceptor;

/// Zumble, a mumble server implementation for FiveM
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Listen address for TCP and UDP connections for mumble voip clients (or other clients that support the mumble protocol)
    #[clap(short, long, value_parser, default_value = "0.0.0.0:64738")]
    listen: String,
    /// Listen address for HTTP connections for the admin api
    #[clap(short, long, value_parser, default_value = "0.0.0.0:8080")]
    http_listen: String,
    /// User for the http server api basic authentification
    #[clap(long, value_parser, default_value = "admin")]
    http_user: String,
    /// Password for the http server api basic authentification
    #[clap(long, value_parser)]
    http_password: String,
    /// Use TLS for the http server (https), will use the same certificate as the mumble server
    #[clap(long)]
    https: bool,
    /// Log http requests to stdout
    #[clap(long)]
    http_log: bool,
    /// Path to the key file for the TLS certificate
    #[clap(long, value_parser, default_value = "key.pem")]
    key: String,
    /// Path to the certificate file for the TLS certificate
    #[clap(long, value_parser, default_value = "cert.pem")]
    cert: String,
}

fn load_certs<P: AsRef<Path>>(path: P) -> io::Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
        .map(|mut certs| certs.drain(..).map(Certificate).collect())
}

fn load_keys<P: AsRef<Path>>(path: P) -> io::Result<Vec<PrivateKey>> {
    pkcs8_private_keys(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))
        .map(|mut keys| keys.drain(..).map(PrivateKey).collect())
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[actix_web_codegen::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let certs = match load_certs(args.cert.as_str()) {
        Ok(certs) => certs,
        Err(e) => {
            tracing::error!("cannot load certificate at path {}: {}", args.cert, e);
            return;
        }
    };

    let mut keys = match load_keys(args.key.as_str()) {
        Ok(k) => k,
        Err(e) => {
            tracing::error!("cannot load key at path {}: {}", args.key, e);
            return;
        }
    };

    let config = match rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, keys.remove(0))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
    {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("cannot create tls config: {}", e);
            return;
        }
    };

    let acceptor = TlsAcceptor::from(Arc::new(config.clone()));

    tracing::info!("tcp/udp server start listening on {}", args.listen);
    tracing::info!("http server start listening on {}", args.http_listen);

    // Simulate 1.2.4 protocol version
    let version = 1 << 16 | 2 << 8 | 4;

    let mut server_version = Version::new();
    server_version.set_os(std::env::consts::FAMILY.to_string());
    server_version.set_os_version(std::env::consts::OS.to_string());
    server_version.set_release(VERSION.to_string());
    server_version.set_version(version);

    let udp_socket = Arc::new(UdpSocket::bind(&args.listen).await.unwrap());
    let state = Arc::new(RwLock::new(ServerState::new(udp_socket.clone())));
    let udp_state = state.clone();

    actix_rt::spawn(async move {
        create_udp_server(version, udp_socket, udp_state).await;
    });

    let clean_state = state.clone();

    actix_rt::spawn(async move {
        clean_loop(clean_state).await;
    });

    let tcp_listener = TcpListener::bind(args.listen.clone()).await.unwrap();

    let mut waiting_list = Vec::new();

    // Create tcp server
    let server = create_tcp_server(tcp_listener, acceptor, server_version, state.clone());
    waiting_list.push(server);

    let http_server = create_http_server(
        args.http_listen,
        config,
        args.https,
        state.clone(),
        args.http_user,
        args.http_password,
        args.http_log,
    );

    if let Some(http_server) = http_server {
        waiting_list.push(http_server);
    }

    match futures::future::try_join_all(waiting_list).await {
        Ok(_) => (),
        Err(e) => {
            tracing::error!("agent error: {}", e);
        }
    }
}
