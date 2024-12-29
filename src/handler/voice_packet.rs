use scc::ebr::Guard;
use scc::HashMap;
use tokio::sync::mpsc::error::TrySendError;

use crate::client::{ClientArc, WeakClient};
use crate::error::DisconnectReason;
use crate::message::ClientMessage;
use crate::state::ServerStateRef;
use crate::voice::{ClientBound, VoicePacket};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::{Handler, MumbleResult};

impl Handler for VoicePacket<ClientBound> {
    async fn handle(&self, state: &ServerStateRef, client: &ClientArc) -> MumbleResult {
        let mute = client.is_muted();

        if mute {
            return Ok(());
        }

        if let VoicePacket::<ClientBound>::Audio { target, session_id, .. } = self {
            // copy the data into an arc so we can reuse the packet for each client

            let listening_clients: HashMap<u32, WeakClient> = HashMap::new();

            match *target {
                // Channel
                0 => {
                    let channel_id = client.channel_id.load(Ordering::Relaxed);

                    let guard = Guard::new();
                    if let Some(channel) = state.channels.peek(&channel_id, &guard) {
                        let guard = Guard::new();

                        for (session_id, client) in channel.clients.iter(&guard) {
                            let _ = listening_clients.insert(*session_id, Arc::downgrade(client));
                        }
                    }
                }
                // Voice target (whisper)
                1..=30 => {
                    let target = client.get_target(*target);

                    if let Some(target) = target {
                        {
                            let guard = Guard::new();
                            for (session, _) in target.sessions.iter(&guard) {
                                let client_guard = Guard::new();
                                if let Some(client) = state.clients.peek(session, &client_guard) {
                                    let _ = listening_clients.insert(*session, Arc::downgrade(client));
                                }
                            }
                        }

                        {
                            let guard = Guard::new();
                            for (channel_id, _) in target.channels.iter(&guard) {
                                let guard = Guard::new();
                                if let Some(target_channel) = state.channels.peek(channel_id, &guard) {
                                    {
                                        let guard = Guard::new();
                                        for (session_id, client) in target_channel.listeners.iter(&guard) {
                                            let _ = listening_clients.insert(*session_id, Arc::downgrade(client));
                                        }
                                    }

                                    {
                                        let guard = Guard::new();
                                        for (session_id, client) in target_channel.clients.iter(&guard) {
                                            let _ = listening_clients.insert(*session_id, Arc::downgrade(client));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Loopback
                31 => {
                    client.send_voice_packet(self.clone()).await?;

                    return Ok(());
                }
                _ => {
                    tracing::error!("invalid voice target: {}", *target);
                }
            }

            // remove the calling client from the session list so we don't have to branch here.
            listening_clients.remove_async(session_id).await;

            listening_clients
                .scan_async(|_k, cl| {
                    if let Some(cl) = cl.upgrade() {
                        if cl.is_deaf() {
                            return;
                        }

                        match cl.publisher.try_send(ClientMessage::SendVoicePacket(self.clone())) {
                            Ok(_) => {}
                            Err(TrySendError::Closed(_)) => {
                                let session_id = cl.session_id;
                                let state = Arc::clone(state);
                                // If we don't have a channel then we should drop the client as the receiving part of the channel got canceled
                                // TODO: have a queue wrapper for state
                                tokio::spawn(async move {
                                    state.disconnect(session_id, DisconnectReason::LostReceivingChannel).await;
                                });
                            }
                            Err(err) => {
                                tracing::error!("error sending voice packet message to {}: {}", cl, err);
                            }
                        }
                    }
                })
                .await;
        }

        Ok(())
    }
}
