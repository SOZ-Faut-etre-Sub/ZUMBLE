use crate::client::ClientArc;
use crate::handler::Handler;
use crate::proto::mumble::Version;
use crate::state::ServerStateRef;

use super::MumbleResult;

impl Handler for Version {
    async fn handle(&self, _state: &ServerStateRef, _client: &ClientArc) -> MumbleResult {
        Ok(())
    }
}
