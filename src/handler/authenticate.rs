use super::MumbleResult;
use crate::{client::ClientArc, handler::Handler, proto::mumble::Authenticate, state::ServerStateRef};

impl Handler for Authenticate {
    async fn handle(&self, _state: &ServerStateRef, _client: &ClientArc) -> MumbleResult {
        // we don't do ACL
        // client.tokens = self.get_tokens().iter().map(|token| token.to_string()).collect();

        Ok(())
    }
}
