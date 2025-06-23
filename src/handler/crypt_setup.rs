use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::CryptSetup;
use crate::sync::RwLock;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
impl Handler for CryptSetup {
    async fn handle(&self, _state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        if self.has_client_nonce() {
            client
                .read_err()
                .await?
                .crypt_state
                .write_err()
                .await?
                .set_decrypt_nonce(self.get_client_nonce());
        } else {
            client.read_err().await?.send_crypt_setup(false).await?;
        }

        Ok(())
    }
}
