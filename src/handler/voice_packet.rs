use scc::HashMap;

use crate::client::ClientRef;
use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::state::ServerStateRef;
use crate::voice::{ClientBound, VoicePacket};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::Handler;

impl Handler for VoicePacket<ClientBound> {
    async fn handle(&self, state: &ServerStateRef, client: &ClientRef) -> Result<(), MumbleError> {
        let mute = client.is_muted();

        if mute {
            return Ok(());
        }

        if let VoicePacket::<ClientBound>::Audio { target, session_id, .. } = self {
            // copy the data into an arc so we can reuse the packet for each client
            let packet = Arc::new(self.clone());

            let listening_clients: HashMap<u32, ClientRef> = HashMap::new();

            match *target {
                // Channel
                0 => {
                    let channel_id = client.channel_id.load(Ordering::Relaxed);
                    let channel_result = state.channels.get_async(&channel_id).await;

                    if let Some(channel) = channel_result {
                        let mut iter = channel.clients.first_entry_async().await;

                        while let Some(client) = iter {
                            let cl = client.get();
                            listening_clients.upsert_async(cl.session_id, Arc::clone(cl)).await;

                            iter = client.next_async().await;
                        }
                    }
                }
                // Voice target (whisper)
                1..=30 => {
                    let target = client.get_target(*target);

                    if let Some(target) = target {
                        let mut iter = target.sessions.first_entry_async().await;

                        while let Some(client) = iter {
                            if let Some(cl) = state.clients.get_async(client.key()).await {
                                listening_clients.upsert_async(cl.session_id, Arc::clone(cl.get())).await;
                            }

                            iter = client.next_async().await;
                        }

                        let mut iter = target.channels.first_entry_async().await;

                        while let Some(channel) = iter {
                            if let Some(ch) = state.channels.get_async(channel.key()).await {
                                let mut listen_iter = ch.listeners.first_entry_async().await;
                                while let Some(listen_cl) = listen_iter {
                                    listening_clients
                                        .upsert_async(listen_cl.session_id, Arc::clone(listen_cl.get()))
                                        .await;

                                    listen_iter = listen_cl.next_async().await;
                                }

                                let mut client_iter = ch.clients.first_entry_async().await;
                                while let Some(client_cl) = client_iter {
                                    listening_clients
                                        .upsert_async(client_cl.session_id, Arc::clone(client_cl.get()))
                                        .await;

                                    client_iter = client_cl.next_async().await;
                                }
                            }
                            iter = channel.next_async().await;
                        }
                    }
                }
                // Loopback
                31 => {
                    client.send_voice_packet(Arc::clone(&packet)).await?;

                    return Ok(());
                }
                _ => {
                    tracing::error!("invalid voice target: {}", *target);
                }
            }

            // remove the calling client from the session list so we don't have to branch here.
            listening_clients.remove_async(session_id).await;

            listening_clients.scan_async(|_k, cl| {
                if cl.is_deaf() {
                    return;
                }

                match cl.publisher.try_send(ClientMessage::SendVoicePacket(Arc::clone(&packet))) {
                    Ok(_) => {}
                    Err(err) => {
                        tracing::error!("error sending voice packet message to {}: {}", cl, err);
                    }
                }

            }).await;
        }

        Ok(())
    }
}
