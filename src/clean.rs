use crate::error::{DisconnectReason, MumbleError};
use crate::server::constants::ConcurrentHashMap;
use crate::state::{ServerState, ServerStateRef};
use std::sync::Arc;
use std::time::Instant;

pub async fn handle_server_tick(state: ServerStateRef) {
    loop {
        tracing::trace!("Running client clean loop");

        match clean_run(&state).await {
            Ok(_) => (),
            Err(e) => {
                tracing::error!("error in clean loop: {}", e);
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

async fn clean_run(state: &ServerState) -> Result<(), MumbleError> {
    let clients_to_remove = ConcurrentHashMap::new();
    let mut clients_to_reset_crypt = Vec::new();

    {
        let mut iter = state.clients.first_entry_async().await;
        while let Some(client_iter) = iter {
            let client = client_iter.get();
            // if we lost our publisher then we should just remove the client, we won't be able to handle anything in this state.
            if client.publisher.is_closed() {
                let _ = clients_to_remove
                    .insert_async(client.session_id, DisconnectReason::LostReceivingChannel)
                    .await;
                iter = client_iter.next_async().await;
                continue;
            }

            let now = Instant::now();

            let since_last_udp = now.duration_since(client.last_udp_ping.load());

            if since_last_udp.as_secs() > 30 {
                // resetting this will cause the client to be removed from the "good socket" list
                clients_to_reset_crypt.push(Arc::clone(client));
                iter = client_iter.next_async().await;
                continue;
            }

            let since_last_tcp = now.duration_since(client.last_tcp_ping.load());

            // if we haven't gotten a tcp ping in 10 seconds (which means they missed 2 mumble
            // pings, or 10 for FiveM) we should be safe to drop the client early.
            if since_last_tcp.as_secs() > 10 {
                let _ = clients_to_remove
                    .insert_async(client.session_id, DisconnectReason::ClientTimedOutTcp)
                    .await;
                iter = client_iter.next_async().await;
                continue;
            }

            let last_good = { client.crypt_state.lock().await.last_good };

            if now.duration_since(last_good).as_millis() > 8000 {
                clients_to_reset_crypt.push(Arc::clone(client))
            }

            iter = client_iter.next_async().await;
        }
    }

    for client in clients_to_reset_crypt {
        let session_id = client.session_id;
        if let Err(e) = state.reset_client_crypt(&client).await {
            tracing::error!("failed to send crypt setup for {}: {:?}", e, session_id);
        } else {
            tracing::info!("Requesting {} crypt be reset", client);
        }
    }

    let mut iter = clients_to_remove.first_entry_async().await;
    while let Some(v) = iter {
        state.disconnect(*v.key(), *v.get()).await;

        iter = v.next_async().await;
    }

    Ok(())
}
