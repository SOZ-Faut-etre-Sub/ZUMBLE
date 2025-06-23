use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Authenticate;
use crate::sync::RwLock;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
impl Handler for Authenticate {
    async fn handle(&self, _state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        client.write_err().await?.tokens = self.get_tokens().iter().map(|token| token.to_string()).collect();

        Ok(())
    }
}
