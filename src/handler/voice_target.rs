use crate::client::ClientRef;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::VoiceTarget;
use crate::state::ServerStateRef;

impl Handler for VoiceTarget {
    async fn handle(&self, _: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        // mumble spec limits the usable voice targets to 1..=30
        if self.get_id() < 1 || self.get_id() >= 31 {
            tracing::error!("invalid voice target id: {}", self.get_id());
            return Err(MumbleError::InvalidVoiceTarget);
        }

        let target_opt = { client.get_target(self.get_id() as u8) };

        // TODO: maybe swap this for raw access (just unwrap) since this shouldn't ever get past
        // the check above
        let target = match target_opt {
            Some(target) => target,
            None => {
                tracing::error!(
                    "{} tried to target voice target {} but the channel didn't exist",
                    client,
                    self.get_id()
                );
                return Ok(());
            }
        };

        target.sessions.clear();
        target.channels.clear();

        for target_item in self.get_targets() {
            for session in target_item.get_session() {
                tracing::debug!("{} is targeting session: {session}", client);
                // we clear this above, we won't run into duplicate inserts.
                let _ = target.sessions.insert(*session);
            }

            if target_item.has_channel_id() {
                tracing::debug!("{} is targeting channel: {}", client, target_item.get_channel_id());
                // we clear this above, we won't run into duplicate inserts.
                let _ = target.channels.insert(target_item.get_channel_id());
            }
        }

        Ok(())
    }
}
