use crate::client::{ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::VoiceTarget;
use crate::state::ServerStateRef;
use async_trait::async_trait;
use std::collections::HashSet;

#[async_trait]
impl Handler for VoiceTarget {
    async fn handle(&self, _: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        // mumble spec limits the usable voice targets to 1..=30
        if self.get_id() < 1 || self.get_id() >= 31 {
            return Ok(());
        }

        let target_opt = { client.get_target(self.get_id() as u8) };

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
            target.sessions.clear();
            target.channels.clear();

            // can't use extend here
            for v in sessions {
                target.sessions.insert(v);
            }

            for v in channels {
                target.channels.insert(v);
            }
        }

        Ok(())
    }
}
