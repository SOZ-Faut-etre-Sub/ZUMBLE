use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::VoiceTarget;
use crate::ServerState;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for VoiceTarget {
    async fn handle(&self, state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        if !self.has_id() {
            return Ok(());
        }

        let target = match client.read().await.get_target((self.get_id() - 1) as usize) {
            Some(target) => target,
            None => {
                log::error!("invalid voice target id: {}", self.get_id());

                return Ok(());
            }
        };

        let mut sessions = HashSet::new();
        let mut channels = HashSet::new();

        for target_item in self.get_targets() {
            for session in target_item.get_session() {
                if state.read().await.clients.get(&session).is_some() {
                    sessions.insert(*session);
                }
            }

            if target_item.has_channel_id() {
                if state.read().await.channels.get(&target_item.get_channel_id()).is_some() {
                    channels.insert(target_item.get_channel_id());
                }
            }
        }

        {
            target.write().await.sessions = sessions;
            target.write().await.channels = channels;
        }

        Ok(())
    }
}
