use super::MumbleResult;
use crate::{client::ClientArc, handler::Handler, proto::mumble::CryptSetup, state::ServerStateRef};

impl Handler for CryptSetup {
    async fn handle(&self, _state: &ServerStateRef, client: &ClientArc) -> MumbleResult {
        if self.has_client_nonce() {
            client.crypt_state.lock().await.set_decrypt_nonce(self.get_client_nonce());
            Ok(())
        } else {
            client.send_crypt_setup(false).await.map_err(anyhow::Error::new)
        }
    }
}
