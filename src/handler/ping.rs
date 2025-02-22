use crate::client::ClientArc;
use crate::handler::Handler;
use crate::proto::MessageKind;
use crate::proto::mumble::Ping;
use crate::state::ServerStateRef;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::MumbleResult;

impl Handler for Ping {
    async fn handle(&self, _state: &ServerStateRef, client: &ClientArc) -> MumbleResult {
        let mut ping = Ping::default();
        ping.set_timestamp(self.get_timestamp());

        {
            client.last_tcp_ping.swap(Instant::now());
        }

        let stats = &client.net_stats;

        stats.udp_packets.store(self.get_udp_packets(), Ordering::Relaxed);
        stats.tcp_packets.store(self.get_tcp_packets(), Ordering::Relaxed);
        stats.udp_ping_avg.store(self.get_udp_ping_avg(), Ordering::Relaxed);
        stats.udp_ping_var.store(self.get_udp_ping_var(), Ordering::Relaxed);
        stats.tcp_ping_avg.store(self.get_tcp_ping_avg(), Ordering::Relaxed);
        stats.tcp_ping_var.store(self.get_tcp_ping_var(), Ordering::Relaxed);

        {
            let mut crypt_state_read = client.crypt_state.lock().await;

            crypt_state_read.remote_good = self.get_good();
            crypt_state_read.remote_late = self.get_late();
            crypt_state_read.remote_lost = self.get_lost();
            crypt_state_read.remote_resync = self.get_resync();

            ping.set_good(crypt_state_read.good);
            ping.set_late(crypt_state_read.late);
            ping.set_lost(crypt_state_read.lost);
            ping.set_resync(crypt_state_read.resync);
        }

        client.send_message(MessageKind::Ping, &ping).await.map_err(anyhow::Error::new)
    }
}
