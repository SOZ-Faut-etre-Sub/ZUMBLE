use crate::client::Client;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::Ping;
use crate::proto::MessageKind;
use crate::ServerState;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for Ping {
    async fn handle(&self, _state: Arc<RwLock<ServerState>>, client: Arc<RwLock<Client>>) -> Result<(), MumbleError> {
        let mut ping = Ping::default();
        ping.set_timestamp(self.get_timestamp());

        {
            let client_read = client.read().await;
            let crypt_state = client_read.crypt_state.read().await;
            ping.set_good(crypt_state.good);
            ping.set_late(crypt_state.late);
            ping.set_lost(crypt_state.lost);
            ping.set_resync(crypt_state.resync);
        }

        client.write().await.send_message(MessageKind::Ping, &ping).await
    }
}
