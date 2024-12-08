use crate::client::ClientRef;
use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::state::ServerStateRef;
use crate::voice::{ClientBound, VoicePacket};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::Handler;

impl Handler for VoicePacket<ClientBound> {
    async fn handle(&self, state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        let mute = client.is_muted();

        if mute {
            return Ok(());
        }

        if let VoicePacket::<ClientBound>::Audio { target, session_id, .. } = self {
            // copy the data into an arc so we can reuse the packet for each client
            let packet = Arc::new(self.clone());

            let mut listening_clients = HashMap::new();

            match *target {
                // Channel
                0 => {
                    let channel_id = client.channel_id.load(Ordering::Relaxed);
                    let channel_result = state.channels.get(&channel_id);

                    if let Some(channel) = channel_result {
                        channel.get_clients().scan(|k, v| {
                            listening_clients.insert(*k, v.clone());
                        });
                    }
                }
                // Voice target (whisper)
                1..=30 => {
                    let target = client.get_target(*target);

                    if let Some(target) = target {
                        target.sessions.scan(|client_id| {
                            let client_result = state.clients.get(client_id);

                            if let Some(client) = client_result {
                                listening_clients.insert(*client_id, client.clone());
                            }
                        });

                        target.channels.scan(|channel_id| {
                            let channel_result = state.channels.get(channel_id);

                            if let Some(channel) = channel_result {
                                channel.get_listeners().scan(|k, v| {
                                    listening_clients.insert(*k, v.clone());
                                });

                                channel.get_clients().scan(|k, v| {
                                    listening_clients.insert(*k, v.clone());
                                });
                            }
                        });
                    }
                }
                // Loopback
                31 => {
                    client.send_voice_packet(packet.clone()).await?;

                    return Ok(());
                }
                _ => {
                    tracing::error!("invalid voice target: {}", *target);
                }
            }

            for client in listening_clients.values() {
                if client.is_deaf() {
                    continue;
                }

                if client.session_id != *session_id {
                    match client.publisher.send(ClientMessage::SendVoicePacket(packet.clone())) {
                        Ok(_) => {}
                        Err(err) => {
                            tracing::error!("error sending voice packet message to {}: {}", client, err);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
