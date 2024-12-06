use crate::client::{Client, ClientRef};
use crate::handler::MessageHandler;
use crate::message::ClientMessage;
use crate::proto::mumble::Version;
use crate::proto::MessageKind;
use crate::server::constants::{MAX_CLIENTS, MAX_MTU};
use crate::state::ServerStateRef;
use actix_server::Server;
use actix_service::fn_service;
use anyhow::{anyhow, Context};
use tokio::io::ReadHalf;
use tokio::io::{self};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio_rustls::{server::TlsStream, TlsAcceptor};

pub fn create_tcp_server(tcp_listener: TcpListener, acceptor: TlsAcceptor, server_version: Version, state: ServerStateRef) -> Server {
    Server::build()
        .listen(
            "mumble-tcp",
            tcp_listener.into_std().expect("cannot create tcp listener"),
            move || {
                let acceptor = acceptor.clone();
                let server_version = server_version.clone();
                let state = state.clone();

                fn_service(move |stream: TcpStream| {
                    let acceptor = acceptor.clone();
                    let server_version = server_version.clone();
                    let state = state.clone();

                    async move {
                        match handle_new_client(acceptor, server_version, state, stream).await {
                            Ok(_) => (),
                            Err(e) => tracing::error!("handle client error: {:?}", e),
                        }

                        Ok::<(), anyhow::Error>(())
                    }
                })
            },
        )
        .expect("cannot create tcp server")
        .run()
}

async fn handle_new_client(
    acceptor: TlsAcceptor,
    server_version: Version,
    state: ServerStateRef,
    stream: TcpStream,
) -> Result<(), anyhow::Error> {
    let cur_clients = state.clients.len();
    let addr = stream.peer_addr()?;
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
    let (tx, rx) = mpsc::channel(MAX_MTU);

    let username = authenticate.get_username().to_string();
    let client = state.add_client(version, authenticate, crypt_state, write, tx);

    tracing::info!("TCP new client {} connected {}", username, addr);

    match client_run(read, rx, state.clone(), client.clone()).await {
        Ok(_) => (),
        Err(e) => tracing::error!("client {} error: {:?}", username, e),
    }

    tracing::info!("client {} disconnected", username);

    state.disconnect(client.session_id);

    Ok(())
}

pub async fn client_run(
    mut read: ReadHalf<TlsStream<TcpStream>>,
    mut receiver: Receiver<ClientMessage>,
    state: ServerStateRef,
    client: ClientRef,
) -> Result<(), anyhow::Error> {
    let codec_version = { state.check_codec().await? };

    if let Some(codec_version) = codec_version {
        client.send_message(MessageKind::CodecVersion, &codec_version).await?;
    }

    {
        client.sync_client_and_channels(&state).await.map_err(|e| {
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
        match MessageHandler::handle(&mut read, &mut receiver, state.clone(), client.clone()).await {
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
