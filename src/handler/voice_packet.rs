use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::voice::{Clientbound, VoicePacket};
use crate::ServerState;
use async_trait::async_trait;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for VoicePacket<Clientbound> {
    async fn handle(&self, state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        let mute = { client.read().await.mute };

        if mute {
            return Ok(());
        }

        if let VoicePacket::<Clientbound>::Audio { target, session_id, .. } = self {
            let mut listening_clients = HashMap::new();

            match *target {
                // Channel
                0 => {
                    let channel_id = { client.read().await.channel_id };

                    if let Some(channel) = { state.read().await.channels.get(&channel_id).cloned() } {
                        {
                            listening_clients.extend(channel.read().await.get_listeners(state.clone()).await);
                        }
                    }
                }
                // Voice target (whisper)
                1..=30 => {
                    let target = { client.read().await.get_target((*target - 1) as usize) };

                    if let Some(target) = target {
                        let target = target.read().await;

                        for client_id in &target.sessions {
                            if let Some(client) = { state.read().await.clients.get(client_id).cloned() } {
                                listening_clients.insert(*client_id, client);
                            }
                        }

                        for channel_id in &target.channels {
                            if let Some(channel) = { state.read().await.channels.get(channel_id).cloned() } {
                                {
                                    listening_clients.extend(channel.read().await.get_listeners(state.clone()).await);
                                }
                            }
                        }
                    }
                }
                // Loopback
                31 => {
                    {
                        client.write().await.send_voice_packet(self).await?;
                    }

                    return Ok(());
                }
                _ => {
                    tracing::error!("invalid voice target: {}", *target);
                }
            }

            // Concurrent voice send
            futures_util::stream::iter(listening_clients)
                .for_each_concurrent(None, |(id, listening_client)| async move {
                    {
                        if id == *session_id {
                            return;
                        }

                        let listening_client_result = { listening_client.write().await.send_voice_packet(self).await };

                        match listening_client_result {
                            Ok(_) => (),
                            Err(err) => tracing::error!("failed to send voice packet to client {}: {}", id, err),
                        }
                    }
                })
                .await;
        }

        Ok(())
    }
}
