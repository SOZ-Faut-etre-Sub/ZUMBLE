use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Version;
use crate::sync::RwLock;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
impl Handler for Version {
    async fn handle(&self, _state: Arc<RwLock<ServerState>>, _client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        Ok(())
    }
}
