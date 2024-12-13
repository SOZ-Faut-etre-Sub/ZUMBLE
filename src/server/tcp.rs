use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use crate::client::{Client, ClientRef};
use crate::handler::MessageHandler;
use crate::message::ClientMessage;
use crate::proto::mumble::Version;
use crate::proto::{expected_message, send_message, MessageKind};
use crate::server::constants::{MAX_BANDWIDTH_IN_BYTES, MAX_CLIENTS};
use crate::state::ServerStateRef;
use anyhow::{anyhow, Context};
use futures::TryFutureExt;
use tokio::io::{self};
use tokio::io::{AsyncWriteExt, ReadHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio_rustls::{server::TlsStream, TlsAcceptor};

pub async fn create_tcp_server(
    tcp_listener: TcpListener,
    acceptor: TlsAcceptor,
    server_version: Version,
    state: ServerStateRef,
) -> anyhow::Result<()> {
    let tls_acceptor = acceptor.clone();

    loop {
        let (mut tcp_stream, _remote_addr) = match tcp_listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{}", e);
                continue;
            }
        };
        let tls_acceptor = tls_acceptor.clone();

        let server_version = server_version.clone();
        let state = state.clone();

        let cur_clients = state.clients.len();
        let addr = tcp_stream.peer_addr()?;

        // if we're over our max client count then we should shut down the tcp stream
        if cur_clients >= MAX_CLIENTS {
            tokio::spawn(async move {
                tcp_stream.shutdown().await.unwrap();
            });
            tracing::info!(
                "{:?} tried to join but the server is at maximum capacity ({}/{})",
                addr,
                cur_clients,
                MAX_CLIENTS
            );
            continue;
        }

        let handle_accept_tls_stream = async move {
            let peer_ip = addr.ip();

            tcp_stream.set_nodelay(true).context("set stream no delay").unwrap();

            let stream = tls_acceptor
                .accept(tcp_stream)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Client TLS connect fail: {:?}", e)));

            const TLS_TIMEOUT: u64 = 5;

            let stream = tokio::time::timeout(Duration::from_secs(TLS_TIMEOUT), stream).map_err(move |_e| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Client TLS handshake timedout after {} seconds", TLS_TIMEOUT),
                )
            });

            let res: Result<TlsStream<TcpStream>, anyhow::Error> = match stream.await {
                Ok(Ok(tls_stream)) => Ok(tls_stream),
                Err(e) => Err(e.into()),
                Ok(Err(e)) => Err(e.into()),
            };

            (res, peer_ip)
        };

        tokio::spawn(async move {
            let (tls_stream, peer_ip) = match handle_accept_tls_stream.await {
                (Ok(tls_stream), peer_ip) => (tls_stream, peer_ip),
                (Err(e), _) => {
                    return Err(e.into())
                }
            };

            handle_new_client(tls_stream, peer_ip, server_version, state).await
        });
    }
}

async fn handle_new_client(
    mut tls_stream: TlsStream<TcpStream>,
    peer_ip: IpAddr,
    server_version: Version,
    state: ServerStateRef,
) -> Result<(), anyhow::Error> {
    let (version, authenticate, crypt_state) = Client::init(&mut tls_stream, server_version).await.context("init client")?;

    let (read, write) = io::split(tls_stream);
    let (tx, rx) = mpsc::channel(MAX_BANDWIDTH_IN_BYTES);

    let username = authenticate.get_username().to_string();
    let client = state.add_client(version, authenticate, crypt_state, write, tx, peer_ip);

    tracing::info!("TCP new client {} connected {}", username, peer_ip);

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
    let codec_version = { state.codec_state.get_codec_version() };

    client.send_message(MessageKind::CodecVersion, &codec_version).await?;

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
