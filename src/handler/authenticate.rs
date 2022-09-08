use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Authenticate;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for Authenticate {
    async fn handle(&self, _state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        client.write().await.tokens = self.get_tokens().iter().map(|token| token.to_string()).collect();

        Ok(())
    }
}
