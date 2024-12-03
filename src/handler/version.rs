use crate::client::{ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Version;
use crate::state::ServerStateRef;

impl Handler for Version {
    async fn handle(&self, _state: ServerStateRef, _client: ClientRef) -> Result<(), MumbleError> {
        Ok(())
    }
}
