use std::net::SocketAddr;

use crate::client::{Client, ClientRef};
use crate::handler::MessageHandler;
use crate::message::ClientMessage;
use crate::proto::mumble::Version;
use crate::proto::{expected_message, send_message, MessageKind};
use crate::server::constants::{MAX_BANDWIDTH_IN_BYTES, MAX_CLIENTS};
use crate::state::ServerStateRef;
use anyhow::{anyhow, Context};
use tokio::io::ReadHalf;
use tokio::io::{self};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio_rustls::{server::TlsStream, TlsAcceptor};

pub async fn create_tcp_server(
    tcp_listener: SocketAddr,
    acceptor: TlsAcceptor,
    server_version: Version,
    state: ServerStateRef,
) -> anyhow::Result<()> {
    let tls_acceptor = acceptor.clone();
    let incoming = TcpListener::bind(tcp_listener).await.expect("Failed to create TCP listener");

    loop {
        let (tcp_stream, _remote_addr) = incoming.accept().await?;
        let tls_acceptor = tls_acceptor.clone();

        let server_version = server_version.clone();
        let state = state.clone();

        tokio::spawn(async move {
            match handle_new_client(tls_acceptor, server_version, state, tcp_stream).await {
                Ok(_) => (),
                Err(e) => tracing::error!("handle client error: {:?}", e),
            }
        });
    }
}

async fn handle_new_client(
    acceptor: TlsAcceptor,
    server_version: Version,
    state: ServerStateRef,
    stream: TcpStream,
) -> Result<(), anyhow::Error> {
    let cur_clients = state.clients.len();
    let addr = stream.peer_addr()?;

    let peer_ip = addr.ip();

    if cur_clients >= MAX_CLIENTS {
        return Err(anyhow!(
            "{:?} tried to join but the server is at maximum capacity ({}/{})",
            addr,
            cur_clients,
            MAX_CLIENTS
        ));
    }

    stream.set_nodelay(true).context("set stream no delay")?;

    let mut stream = acceptor.accept(stream).await.context("accept tls")?;

    let (version, authenticate, crypt_state) = Client::init(&mut stream, server_version).await.context("init client")?;

    let (read, write) = io::split(stream);
    let (tx, rx) = mpsc::channel(MAX_BANDWIDTH_IN_BYTES);

    let username = authenticate.get_username().to_string();
    let client = state.add_client(version, authenticate, crypt_state, write, tx, peer_ip);

    tracing::info!("TCP new client {} connected {}", username, addr);

    let state_cl = state.clone();
    let client_cl = client.clone();
    match client_run(read, rx, &state_cl, &client_cl).await {
        Ok(_) => (),
        Err(e) => (),
    }

    tracing::info!("client {} disconnected", username);

    state_cl.disconnect(client.session_id).await;

    Ok(())
}

pub async fn client_run(
    mut read: ReadHalf<TlsStream<TcpStream>>,
    mut receiver: Receiver<ClientMessage>,
    state: &ServerStateRef,
    client: &ClientRef,
) -> Result<(), anyhow::Error> {
    let codec_version = { state.check_codec().await? };

    if let Some(codec_version) = codec_version {
        client.send_message(MessageKind::CodecVersion, &codec_version).await?;
    }

    {
        client.sync_client_and_channels(state).await.map_err(|e| {
            tracing::error!("init client error during channel sync: {:?}", e);

            e
        })?;
        client.send_my_user_state().await?;
        client.send_server_sync().await?;
        client.send_server_config().await?;
    }

    let user_state = { client.get_user_state() };

    {
        match state.broadcast_message(MessageKind::UserState, &user_state) {
            Ok(_) => (),
            Err(e) => tracing::error!("failed to send user state: {:?}", e),
        }
    }

    loop {
        match MessageHandler::handle(&mut read, &mut receiver, state, client).await {
            Ok(_) => (),
            Err(e) => {
                if e.is::<io::Error>() {
                    let ioerr = e.downcast::<io::Error>().unwrap();

                    // avoid error for client disconnect
                    if ioerr.kind() == io::ErrorKind::UnexpectedEof {
                        return Ok(());
                    }

                    return Err(ioerr.into());
                }

                return Err(e);
            }
        }
    }
}
