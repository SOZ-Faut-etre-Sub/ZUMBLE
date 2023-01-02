use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::VoiceTarget;
use crate::sync::RwLock;
use crate::ServerState;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;

#[async_trait]
impl Handler for VoiceTarget {
    async fn handle(&self, _: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        if !self.has_id() {
            return Ok(());
        }

        let target_opt = { client.read_err().await?.get_target((self.get_id() - 1) as usize) };

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
            target.write_err().await?.sessions = sessions;
            target.write_err().await?.channels = channels;
        }

        Ok(())
    }
}
