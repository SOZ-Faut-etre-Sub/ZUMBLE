use rustls::ServerConfig;

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
mod target;
mod varint;
mod voice;

use crate::clean::handle_server_tick;
use crate::http::create_http_server;
use crate::proto::mumble::Version;
use crate::server::{create_tcp_server, create_udp_server};
use crate::state::ServerState;

use axum_server::tls_rustls::RustlsConfig;
use clap::Parser;
use rcgen::{date_time_ymd, CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P384_SHA384};
use rustls::crypto::{self, CryptoProvider};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::PrivateKeyDer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinSet;
use tokio_rustls::rustls::{self};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

/// Zumble, a mumble server implementation for FiveM
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None, disable_help_flag = true)]
struct Args {
    #[clap(long, action = clap::ArgAction::HelpLong)]
    help: Option<bool>,
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
    #[clap(long, value_parser, default_value = None)]
    http_password: Option<String>,
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

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() {
    // let console_layer = console_subscriber::spawn();
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let config = Arc::new(generate_rustls_cert());

    let http_config = RustlsConfig::from_config(Arc::clone(&config));

    let acceptor = TlsAcceptor::from(Arc::clone(&config));

    // ignore the fact that `0` does nothing here
    #[allow(clippy::identity_op)]
    // Simulate 1.4.0 protocol version
    let version = 1 << 16 | 4 << 8 | 0;

    let mut server_version = Version::new();
    server_version.set_os(std::env::consts::FAMILY.to_string());
    server_version.set_os_version(std::env::consts::OS.to_string());
    server_version.set_release(VERSION.to_string());
    server_version.set_version(version);

    let mut set = JoinSet::new();

    let std_socket = std::net::UdpSocket::bind(&args.listen).unwrap();
    std_socket.set_nonblocking(true).unwrap();

    let socket = UdpSocket::from_std(std_socket).unwrap();

    let udp_socket = Arc::new(socket);

    let state = Arc::new(ServerState::new(udp_socket.clone()));
    let udp_state = state.clone();

    tracing::info!("tcp/udp server start listening on {}", args.listen);

    let cancelation_token = CancellationToken::new();

    set.spawn(async move {
        create_udp_server(version, udp_socket, udp_state, cancelation_token.clone()).await;
    });

    let clean_state = state.clone();

    set.spawn(async move {
        handle_server_tick(clean_state).await;
    });

    let tcp_addr: SocketAddr = args.listen.parse().expect("Got invalid data for 'listen', it was not a usable ip");

    let tcp_listener = TcpListener::bind(tcp_addr).await.expect("failed to bind to tcp address");
    let tcp_state = state.clone();
    // Create tcp server
    set.spawn(async move {
        match create_tcp_server(tcp_listener, acceptor, server_version, tcp_state).await {
            Ok(_) => (),
            Err(e) => {
                tracing::error!("{}", e);
            }
        }
    });

    let http_server = create_http_server(state.clone(), args.http_user, args.http_password);

    if let Some(http_server) = http_server {
        tracing::info!("http server start listening on {}", args.http_listen);
        set.spawn(async move {
            let socket_addr: SocketAddr = args.http_listen.parse().expect("Invalid socket for http_listen");
            if args.https {
                axum_server::bind_rustls(socket_addr, http_config)
                    .serve(http_server.into_make_service())
                    .await
                    .unwrap();
            } else {
                axum_server::bind(socket_addr).serve(http_server.into_make_service()).await.unwrap();
            }
        });
    } else {
        tracing::info!("http server not started, no auth password provided");
    }

    while let Some(_) = set.join_next().await {}
}

fn generate_rustls_cert() -> ServerConfig {
    CryptoProvider::install_default(crypto::ring::default_provider()).expect("failed to install ring crypto provider");

    // This doesn't really matter for us as this isn't checked for FiveM
    let cert = vec!["localhost".to_string()];

    // TODO: Maybe store this? not really entirely that useful but who knows.
    let generate_key = KeyPair::generate_for(&PKCS_ECDSA_P384_SHA384);
    let key_pair = generate_key.unwrap();

    let mut cert = CertificateParams::new(cert).expect("Unable to generate certificate");
    // we need to change our time to be something sensible, botan will freak out if this is greater
    // than 2200 (by default it gens to 4096)
    cert.not_after = date_time_ymd(2100, 1, 1);

    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, "Mumble self signed cert");
    cert.distinguished_name = distinguished_name;

    let cert = cert.self_signed(&key_pair).unwrap();

    let pem = key_pair.serialize_pem();

    let key_der = PrivateKeyDer::from_pem_slice(pem.as_bytes()).expect("Couldn't make key_der");

    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key_der)
        .expect("Unable to create tlsconfig")
}
