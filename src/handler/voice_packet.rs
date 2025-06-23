use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::message::ClientMessage;
use crate::sync::RwLock;
use crate::voice::{Clientbound, VoicePacket};
use crate::ServerState;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[async_trait]
impl Handler for VoicePacket<Clientbound> {
    async fn handle(&self, state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        let mute = { client.read_err().await?.mute };

        if mute {
            return Ok(());
        }

        let mute_all = { state.read_err().await?.mute_all.load(Ordering::Relaxed) };

        if mute_all {
            return Ok(());
        }

        if let VoicePacket::<Clientbound>::Audio { target, session_id, .. } = self {
            let mut listening_clients = HashMap::new();

            match *target {
                // Channel
                0 => {
                    let channel_id = { client.read_err().await?.channel_id.load(Ordering::Relaxed) };
                    let channel_result = { state.read_err().await?.channels.get(&channel_id).cloned() };

                    if let Some(channel) = channel_result {
                        {
                            listening_clients.extend(channel.read_err().await?.get_listeners(state.clone()).await);
                        }
                    }
                }
                // Voice target (whisper)
                1..=30 => {
                    let target = { client.read_err().await?.get_target((*target - 1) as usize) };

                    if let Some(target) = target {
                        let target = target.read_err().await?;

                        for client_id in &target.sessions {
                            let client_result = { state.read_err().await?.clients.get(client_id).cloned() };

                            if let Some(client) = client_result {
                                listening_clients.insert(*client_id, client);
                            }
                        }

                        for channel_id in &target.channels {
                            let channel_result = { state.read_err().await?.channels.get(channel_id).cloned() };

                            if let Some(channel) = channel_result {
                                {
                                    listening_clients.extend(channel.read_err().await?.get_listeners(state.clone()).await);
                                }
                            }
                        }
                    }
                }
                // Loopback
                31 => {
                    {
                        client.read_err().await?.send_voice_packet(self.clone()).await?;
                    }

                    return Ok(());
                }
                _ => {
                    tracing::error!("invalid voice target: {}", *target);
                }
            }

            for client in listening_clients.values() {
                {
                    let client_read = client.read_err().await?;

                    if client_read.deaf {
                        continue;
                    }

                    if client_read.session_id != *session_id {
                        match client_read.publisher.try_send(ClientMessage::SendVoicePacket(self.clone())) {
                            Ok(_) => {}
                            Err(err) => {
                                tracing::error!(
                                    "error sending voice packet message to {}: {}",
                                    client_read.authenticate.get_username(),
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
