use crate::error::DecryptError;
use crate::message::ClientMessage;
use crate::state::ServerStateRef;
use crate::voice::VoicePacket;

use anyhow::anyhow;

use byteorder::{ReadBytesExt, WriteBytesExt};
use bytes::BytesMut;
use std::net::SocketAddr;
use std::sync::Arc;
use std::io::Cursor;
use tokio::net::UdpSocket;

use super::constants::{MAX_BANDWIDTH, MAX_CLIENTS};

pub async fn create_udp_server(protocol_version: u32, socket: Arc<UdpSocket>, state: ServerStateRef) {
    loop {
        match udp_server_run(protocol_version, socket.clone(), state.clone()).await {
            Ok(_) => (),
            Err(e) => tracing::error!("udp server error: {:?}", e),
        }
    }
}

async fn udp_server_run(protocol_version: u32, socket: Arc<UdpSocket>, state: ServerStateRef) -> Result<(), anyhow::Error> {
    let mut buffer = BytesMut::zeroed(1024);
    let (size, addr) = socket.recv_from(&mut buffer).await?;
    buffer.resize(size, 0);

    tokio::spawn(async move {
        match handle_packet(buffer, size, addr, protocol_version, socket, state).await {
            Ok(_) => (),
            Err(e) => tracing::error!("udp server handle packet error: {:?}", e),
        }
    });

    Ok(())
}

async fn handle_packet(
    mut buffer: BytesMut,
    size: usize,
    addr: SocketAddr,
    protocol_version: u32,
    socket: Arc<UdpSocket>,
    state: ServerStateRef,
) -> Result<(), anyhow::Error> {
    if size <= 1 {
        return Err(anyhow!("Invalid packet"));
    }
    let mut cursor = Cursor::new(&buffer[..size]);
    let kind = cursor.read_u32::<byteorder::LittleEndian>()?;

    // respond to the server list ping packet
    if size == 12 && kind == 0 {
        let timestamp = cursor.read_u64::<byteorder::LittleEndian>()?;

        // TODO: actually read version and follow the mumble spec for using UDP protobufs here
        let mut send = Cursor::new(vec![0u8; 24]);
        // server version
        send.write_u32::<byteorder::BigEndian>(protocol_version)?;
        // timestamp
        send.write_u64::<byteorder::LittleEndian>(timestamp)?;
        // user count
        send.write_u32::<byteorder::BigEndian>(state.clients.len() as u32)?;
        // max user count
        send.write_u32::<byteorder::BigEndian>(MAX_CLIENTS as u32)?;
        // max bandwidth per user
        send.write_u32::<byteorder::BigEndian>(MAX_BANDWIDTH)?;

        socket.send_to(send.get_ref().as_slice(), addr).await?;

        crate::metrics::MESSAGES_TOTAL
            .with_label_values(&["udp", "input", "PingAnonymous"])
            .inc();

        crate::metrics::MESSAGES_BYTES
            .with_label_values(&["udp", "input", "PingAnonymous"])
            .inc_by(size as u64);

        return Ok(());
    }

    // This breaks when people are using VPN's, should add an option to use it for servers getting
    // hit by DDoS's
    // if !state.clients_by_peer.contains(&addr.ip()) {
    //     tracing::warn!(
    //         "UPP: User tried to connect with addr: {} but they didn't connect via TCP before.",
    //         addr
    //     );
    //     return Err(anyhow!("Not a valid peer"));
    // }

    let client_opt = state.get_client_by_socket(&addr);

    let (client, packet) = match client_opt {
        Some(client) => {
            // Send decrypt packet

            let decrypt_result = {
                let mut crypt_state = client.crypt_state.lock();
                crypt_state.decrypt(&mut buffer)
            };

            match decrypt_result {
                Ok(p) => (client, p),
                Err(err) => {
                    tracing::warn!("client {} decrypt error: {}", client, err);

                    crate::metrics::MESSAGES_TOTAL
                        .with_label_values(&["udp", "input", "VoicePacket"])
                        .inc();

                    crate::metrics::MESSAGES_BYTES
                        .with_label_values(&["udp", "input", "VoicePacket"])
                        .inc_by(size as u64);

                    let restart_crypt = match err {
                        DecryptError::Late => {
                            let late = { client.crypt_state.lock().late };

                            late > 100
                        }
                        DecryptError::Repeat => false,
                        _ => true,
                    };

                    if restart_crypt {
                        tracing::error!("client {} udp decrypt error: {}, reset crypt setup", client, err);

                        if let Err(e) = state.reset_client_crypt(client.clone()).await {
                            tracing::error!("failed to send crypt setup: {:?}", e);
                        }
                    }

                    return Ok(());
                }
            }
        }
        None => {
            let (client_opt, packet_opt) = state.find_client_with_decrypt(&mut buffer, addr).await?;

            match (client_opt, packet_opt) {
                (Some(client), Some(packet)) => {
                    tracing::info!("UPD connected client {} on {}", client, addr);

                    (client, packet)
                }
                _ => {
                    // don't log if we've done it recently
                    // if let Ok(Some((_, _))) = state.logs.put(addr, ()) {
                    tracing::error!("unknown client from address {}", addr);
                    // }

                    crate::metrics::UNKNOWN_MESSAGES_TOTAL
                        .with_label_values(&["udp", "input", "UnknownPackets"])
                        .inc();

                    crate::metrics::UNKNOWN_MESSAGES_BYTES
                        .with_label_values(&["udp", "input", "UnknownPacket"])
                        .inc_by(size as u64);

                    return Ok(());
                }
            }
        }
    };

    let session_id = client.session_id;
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
                let mut crypt = client.crypt_state.lock();
                crypt.encrypt(&client_packet, &mut dest);
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

            let send_client_packet = { client.publisher.try_send(ClientMessage::RouteVoicePacket(client_packet)) };

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
