use crate::client::{Client, ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::VoiceTarget;
use crate::state::ServerStateRef;
use crate::ServerState;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for VoiceTarget {
    async fn handle(&self, _: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        if !self.has_id() {
            return Ok(());
        }

        let target_opt = { client.get_target((self.get_id() - 1) as usize) };

        let target = match target_opt {
            Some(target) => target,
            None => {
                tracing::error!("invalid voice target id: {}", self.get_id());

                return Ok(());
            }
        };

        let mut sessions = HashSet::new();
        let mut channels = HashSet::new();

        for target_item in self.get_targets() {
            for session in target_item.get_session() {
                sessions.insert(*session);
            }

            if target_item.has_channel_id() {
                channels.insert(target_item.get_channel_id());
            }
        }

        {
            target.write().await.sessions = sessions;
            target.write().await.channels = channels;
        }

        Ok(())
    }
}
