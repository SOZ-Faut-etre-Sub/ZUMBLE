use crate::client::{Client, ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::UserState;
use crate::state::ServerStateRef;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for UserState {
    async fn handle(&self, state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        let session_id = { client.session_id };

        if self.get_session() != session_id {
            return Ok(());
        }

        {
            client.update(self);
        }

        if self.has_channel_id() {
            let leave_channel_id = match state.set_client_channel(client.clone(), self.get_channel_id()).await {
                Ok(l) => l,
                Err(_) => None,
            };

            if let Some(leave_channel_id) = leave_channel_id {
                {
                    state.channels.remove(&leave_channel_id);
                }
            }
        }

        for channel_id in self.get_listening_channel_add() {
            {
                if let Some(channel) = state.channels.get(channel_id) {
                    channel.listeners.insert(session_id, client.clone());
                }
            }
        }

        for channel_id in self.get_listening_channel_remove() {
            {
                if let Some(channel) = state.channels.get(channel_id) {
                    channel.listeners.remove(&session_id);
                }
            }
        }

        Ok(())
    }
}
