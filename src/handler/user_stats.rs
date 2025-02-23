use std::sync::atomic::Ordering;
use std::sync::Arc;

use protobuf::Clear;
use scc::ebr::Guard;

use crate::client::ClientArc;
use crate::handler::Handler;
use crate::proto::mumble::UserStats;
use crate::proto::MessageKind;
use crate::state::ServerStateRef;

use super::MumbleResult;

impl Handler for UserStats {
    async fn handle(&self, _state: &ServerStateRef, client: &ClientArc) -> MumbleResult {

        // we don't have ACL so we'll just always return true here for right now.
        let include_extended_info = self.get_session() == client.session_id || true;

        // we don't support certs 
        let mut include_cert_info = false;
        let include_crypt_stats = include_extended_info;

        if self.get_stats_only() {
            include_cert_info = false;
        }

        let mut msg = UserStats::new();

        msg.set_session(client.session_id);

        if include_cert_info {
            // TODO: Setup certs
        }

        if include_crypt_stats {
            let crypt_info = client.crypt_state.lock().await;
            let crypto_stats = msg.mut_from_client();
            crypto_stats.set_good(crypt_info.remote_good);
            crypto_stats.set_late(crypt_info.remote_late);
            crypto_stats.set_lost(crypt_info.remote_lost);
            crypto_stats.set_resync(crypt_info.remote_resync);


            let crypto_stats = msg.mut_from_server();

            crypto_stats.set_good(crypt_info.remote_good);
            crypto_stats.set_late(crypt_info.remote_late);
            crypto_stats.set_lost(crypt_info.remote_lost);
            crypto_stats.set_resync(crypt_info.remote_resync);
        }

        let stats = &client.net_stats;

        msg.set_udp_packets(stats.udp_packets.load(Ordering::Relaxed));
        msg.set_tcp_packets(stats.tcp_packets.load(Ordering::Relaxed));
        msg.set_udp_ping_avg(stats.udp_ping_avg.load(Ordering::Relaxed));
        msg.set_tcp_ping_var(stats.udp_ping_var.load(Ordering::Relaxed));
        msg.set_tcp_ping_avg(stats.tcp_ping_avg.load(Ordering::Relaxed));
        msg.set_tcp_ping_var(stats.tcp_ping_var.load(Ordering::Relaxed));


        client.send_message(MessageKind::UserStats, &msg).await.map_err(anyhow::Error::new)
    }
}
