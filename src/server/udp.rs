use crate::error::DecryptError;
use crate::message::ClientMessage;
use crate::sync::RwLock;
use crate::voice::VoicePacket;
use crate::ServerState;
use anyhow::Context;
use byteorder::{ReadBytesExt, WriteBytesExt};
use bytes::BytesMut;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;

pub async fn create_udp_server(protocol_version: u32, socket: Arc<UdpSocket>, state: Arc<RwLock<ServerState>>) {
    loop {
        match udp_server_run(protocol_version, socket.clone(), state.clone()).await {
            Ok(_) => (),
            Err(e) => tracing::error!("udp server error: {:?}", e),
        }
    }
}

async fn udp_server_run(protocol_version: u32, socket: Arc<UdpSocket>, state: Arc<RwLock<ServerState>>) -> Result<(), anyhow::Error> {
    let mut buffer = BytesMut::zeroed(1024);
    let mut dead_clients = HashMap::new();
    let (size, addr) = socket.recv_from(&mut buffer).await?;
    buffer.resize(size, 0);

    let mut cursor = Cursor::new(&buffer[..size]);
    let kind = cursor.read_u32::<byteorder::BigEndian>()?;

    if size == 12 && kind == 0 {
        let timestamp = cursor.read_u64::<byteorder::LittleEndian>()?;

        let mut send = Cursor::new(vec![0u8; 24]);
        send.write_u32::<byteorder::BigEndian>(protocol_version)?;
        send.write_u64::<byteorder::LittleEndian>(timestamp)?;
        send.write_u32::<byteorder::BigEndian>(0)?;
        send.write_u32::<byteorder::BigEndian>(250)?;
        send.write_u32::<byteorder::BigEndian>(72000)?;

        socket.send_to(send.get_ref().as_slice(), addr).await?;

        crate::metrics::MESSAGES_TOTAL
            .with_label_values(&["udp", "input", "PingAnonymous"])
            .inc();

        crate::metrics::MESSAGES_BYTES
            .with_label_values(&["udp", "input", "PingAnonymous"])
            .inc_by(size as u64);

        return Ok(());
    }

    // keep dead clients for 20 seconds
    if let Some(dead) = dead_clients.get(&addr) {
        if Instant::now().duration_since(*dead).as_secs() < 20 {
            return Ok(());
        }
    }

    let client_opt = { state.read_err().await?.get_client_by_socket(&addr) };

    let (client, packet) = match client_opt {
        Some(client) => {
            let decrypt_result = {
                client
                    .read_err()
                    .await?
                    .crypt_state
                    .write_err()
                    .await
                    .context("decrypt voice packet")?
                    .decrypt(&mut buffer)
            };

            match decrypt_result {
                Ok(p) => (client, p),
                Err(err) => {
                    let username = { client.read_err().await?.authenticate.get_username().to_string() };
                    tracing::warn!("client {} decrypt error: {}", username, err);

                    crate::metrics::MESSAGES_TOTAL
                        .with_label_values(&["udp", "input", "VoicePacket"])
                        .inc();

                    crate::metrics::MESSAGES_BYTES
                        .with_label_values(&["udp", "input", "VoicePacket"])
                        .inc_by(size as u64);

                    let restart_crypt = match err {
                        DecryptError::Late => {
                            let late = { client.read_err().await?.crypt_state.read_err().await?.late };

                            late > 100
                        }
                        DecryptError::Repeat => false,
                        _ => true,
                    };

                    if restart_crypt {
                        tracing::error!("client {} udp decrypt error: {}, reset crypt setup", username, err);

                        let send_crypt_setup = { client.read_err().await?.send_crypt_setup(true).await };

                        if let Err(e) = send_crypt_setup {
                            tracing::error!("failed to send crypt setup: {:?}", e);
                        }

                        let client_address = { client.read_err().await?.udp_socket_addr.clone() };

                        // Remove socket address from client
                        if let Some(address) = client_address {
                            {
                                state
                                    .write_err()
                                    .await
                                    .context("remove client by socket")?
                                    .remove_client_by_socket(&address)
                            };

                            {
                                client.write_err().await.context("set udp socket to null")?.udp_socket_addr = None;
                            };
                        }
                    }

                    return Ok(());
                }
            }
        }
        None => {
            let (client_opt, packet_opt, address_to_remove) = { state.read_err().await?.find_client_for_packet(&mut buffer).await? };

            for address in address_to_remove {
                {
                    state
                        .write_err()
                        .await
                        .context("remove client by socket when searching for one")?
                        .remove_client_by_socket(&address)
                };
            }

            match (client_opt, packet_opt) {
                (Some(client), Some(packet)) => {
                    {
                        tracing::info!(
                            "UPD connected client {} on {}",
                            client.read_err().await?.authenticate.get_username(),
                            addr
                        );
                    }

                    {
                        state
                            .write_err()
                            .await
                            .context("set client socket")?
                            .set_client_socket(client.clone(), addr)
                            .await?;
                    }

                    (client, packet)
                }
                _ => {
                    tracing::error!("unknown client from address {}", addr);

                    dead_clients.insert(addr, Instant::now());

                    crate::metrics::MESSAGES_TOTAL
                        .with_label_values(&["udp", "input", "VoicePacket"])
                        .inc();

                    crate::metrics::MESSAGES_BYTES
                        .with_label_values(&["udp", "input", "VoicePacket"])
                        .inc_by(size as u64);

                    return Ok(());
                }
            }
        }
    };

    // remove from dead clients if exists
    if dead_clients.contains_key(&addr) {
        dead_clients.remove(&addr);
    }

    let session_id = { client.read_err().await?.session_id };
    let client_packet = packet.into_client_bound(session_id);

    match &client_packet {
        VoicePacket::Ping { .. } => {
            crate::metrics::MESSAGES_TOTAL
                .with_label_values(&["udp", "input", "VoicePing"])
                .inc();

            crate::metrics::MESSAGES_BYTES
                .with_label_values(&["udp", "input", "VoicePing"])
                .inc_by(size as u64);

            let mut dest = BytesMut::new();

            {
                client
                    .read_err()
                    .await?
                    .crypt_state
                    .write_err()
                    .await
                    .context("encrypt voice packet")?
                    .encrypt(&client_packet, &mut dest);
            }

            let buf = &dest.freeze()[..];

            match socket.send_to(buf, addr).await {
                Ok(_) => {
                    crate::metrics::MESSAGES_TOTAL
                        .with_label_values(&["udp", "output", "VoicePing"])
                        .inc();

                    crate::metrics::MESSAGES_BYTES
                        .with_label_values(&["udp", "output", "VoicePing"])
                        .inc_by(buf.len() as u64);
                }
                Err(err) => {
                    tracing::error!("cannot send ping udp packet: {}", err);
                }
            }
        }
        _ => {
            crate::metrics::MESSAGES_TOTAL
                .with_label_values(&["udp", "input", "VoicePacket"])
                .inc();

            crate::metrics::MESSAGES_BYTES
                .with_label_values(&["udp", "input", "VoicePacket"])
                .inc_by(size as u64);

            let send_client_packet = {
                client
                    .read_err()
                    .await?
                    .publisher
                    .send(ClientMessage::RouteVoicePacket(client_packet))
                    .await
            };

            match send_client_packet {
                Ok(_) => (),
                Err(err) => {
                    tracing::error!("cannot send voice packet to client: {}", err);
                }
            }
        }
    }

    Ok(())
}
