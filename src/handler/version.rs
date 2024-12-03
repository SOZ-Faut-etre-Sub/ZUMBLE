use crate::client::{Client, ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Version;
use crate::state::ServerStateRef;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for Version {
    async fn handle(&self, _state: ServerStateRef, _client: ClientRef) -> Result<(), MumbleError> {
        Ok(())
    }
}
