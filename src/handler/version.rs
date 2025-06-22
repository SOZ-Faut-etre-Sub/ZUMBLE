use super::MumbleResult;
use crate::{client::ClientArc, handler::Handler, proto::mumble::Version, state::ServerStateRef};

impl Handler for Version {
    async fn handle(&self, _state: &ServerStateRef, _client: &ClientArc) -> MumbleResult {
        Ok(())
    }
}
