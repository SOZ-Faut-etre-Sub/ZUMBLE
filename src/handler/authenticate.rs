use crate::client::ClientRef;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Authenticate;
use crate::state::ServerStateRef;

impl Handler for Authenticate {
    async fn handle(&self, _state: ServerStateRef, _client: ClientRef) -> Result<(), MumbleError> {
        // we don't do ACL
        // client.tokens = self.get_tokens().iter().map(|token| token.to_string()).collect();

        Ok(())
    }
}
