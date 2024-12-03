use crate::client::{ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Ping;
use crate::proto::MessageKind;
use crate::state::ServerStateRef;
use async_trait::async_trait;
use std::time::Instant;

#[async_trait]
impl Handler for Ping {
    async fn handle(&self, _state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        let mut ping = Ping::default();
        ping.set_timestamp(self.get_timestamp());

        let crypt_state = { client.crypt_state.clone() };

        {
            *client.last_ping.write().await = Instant::now();
        }

        {
            let crypt_state_read = crypt_state.read().await;
            ping.set_good(crypt_state_read.good);
            ping.set_late(crypt_state_read.late);
            ping.set_lost(crypt_state_read.lost);
            ping.set_resync(crypt_state_read.resync);
        }

        client.send_message(MessageKind::Ping, &ping).await
    }
}
