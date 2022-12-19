use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::UserState;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for UserState {
    async fn handle(&self, state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        let session_id = { client.read().await.session_id };

        if self.get_session() != session_id {
            return Ok(());
        }

        {
            client.write().await.update(&self);
        }

        if self.has_channel_id() {
            let leave_channel_id = match state.read().await.set_client_channel(client.clone(), self.get_channel_id()).await {
                Ok(l) => l,
                Err(_) => None,
            };

            if let Some(leave_channel_id) = leave_channel_id {
                {
                    state.write().await.channels.remove(&leave_channel_id);
                }
            }
        }

        let session_id = { client.read().await.session_id };

        for channel_id in self.get_listening_channel_add() {
            {
                if let Some(channel) = state.read().await.channels.get(channel_id) {
                    channel.write().await.listeners.insert(session_id);
                }
            }
        }

        for channel_id in self.get_listening_channel_remove() {
            {
                if let Some(channel) = state.read().await.channels.get(channel_id) {
                    channel.write().await.listeners.remove(&session_id);
                }
            }
        }

        Ok(())
    }
}
