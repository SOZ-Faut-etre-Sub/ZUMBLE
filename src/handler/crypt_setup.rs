use crate::client::ClientRef;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::CryptSetup;
use crate::state::ServerStateRef;

impl Handler for CryptSetup {
    async fn handle(&self, _state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        if self.has_client_nonce() {
            client.crypt_state.lock().set_decrypt_nonce(self.get_client_nonce());
            Ok(())
        } else {
            client.send_crypt_setup(false).await
        }
    }
}
