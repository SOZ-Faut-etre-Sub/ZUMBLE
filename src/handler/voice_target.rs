use crate::client::{ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::VoiceTarget;
use crate::state::ServerStateRef;

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


        for target_item in self.get_targets() {
            target.sessions.clear();
            for session in target_item.get_session() {
                // we clear this above, we won't run into duplicate inserts.
                let _ = target.sessions.insert(*session);
            }

            target.channels.clear();
            if target_item.has_channel_id() {
                // we clear this above, we won't run into duplicate inserts.
                let _ = target.channels.insert(target_item.get_channel_id());
            }
        }

        Ok(())
    }
}
