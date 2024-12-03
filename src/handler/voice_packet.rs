use crate::client::{ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::message::ClientMessage;
use crate::state::ServerStateRef;
use crate::voice::{Clientbound, VoicePacket};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

#[async_trait]
impl Handler for VoicePacket<Clientbound> {
    async fn handle(&self, state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        let mute = client.is_muted();

        if mute {
            return Ok(());
        }

        if let VoicePacket::<Clientbound>::Audio { target, session_id, .. } = self {
            let mut listening_clients = HashMap::new();

            match *target {
                // Channel
                0 => {
                    let channel_id = { client.channel_id.load(Ordering::Relaxed) };
                    let channel_result = { state.channels.get(&channel_id) };

                    if let Some(channel) = channel_result {
                        {
                            for data in channel.get_listeners().iter() {
                                listening_clients.insert(*data.key(), data.value().clone());
                            }
                        }
                    }
                }
                // Voice target (whisper)
                1..=30 => {
                    let target = { client.get_target(*target) };

                    if let Some(target) = target {
                        for client_id in target.sessions.iter() {
                            let client_result = { state.clients.get(&client_id) };

                            if let Some(client) = client_result {
                                listening_clients.insert(*client_id.key(), client.value().clone());
                            }
                        }

                        for channel_id in target.channels.iter() {
                            let channel_result = { state.channels.get(&channel_id) };

                            if let Some(channel) = channel_result {
                                {
                                    for data in channel.get_listeners().iter() {
                                        listening_clients.insert(*data.key(), data.value().clone());
                                    }

                                    for data in channel.get_clients().iter() {
                                        listening_clients.insert(*data.key(), data.value().clone());
                                    }
                                }
                            }
                        }
                    }
                }
                // Loopback
                31 => {
                    {
                        client.send_voice_packet(self.clone()).await?;
                    }

                    return Ok(());
                }
                _ => {
                    tracing::error!("invalid voice target: {}", *target);
                }
            }

            for client in listening_clients.values() {
                {
                    if client.is_deaf() {
                        continue;
                    }

                    if client.session_id != *session_id {
                        match client.publisher.try_send(ClientMessage::SendVoicePacket(self.clone())) {
                            Ok(_) => {}
                            Err(err) => {
                                tracing::error!(
                                    "error sending voice packet message to {}: {}",
                                    client.authenticate.get_username(),
                                    err
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
