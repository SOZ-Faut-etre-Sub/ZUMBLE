use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Ping;
use crate::proto::MessageKind;
use crate::sync::RwLock;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

#[async_trait]
impl Handler for Ping {
    async fn handle(&self, _state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        let mut ping = Ping::default();
        ping.set_timestamp(self.get_timestamp());

        let crypt_state = { client.read_err().await?.crypt_state.clone() };

        {
            *client.read_err().await?.last_ping.write_err().await? = Instant::now();
        }

        {
            let crypt_state_read = crypt_state.read_err().await?;
            ping.set_good(crypt_state_read.good);
            ping.set_late(crypt_state_read.late);
            ping.set_lost(crypt_state_read.lost);
            ping.set_resync(crypt_state_read.resync);
        }

        client.read_err().await?.send_message(MessageKind::Ping, &ping).await
    }
}
