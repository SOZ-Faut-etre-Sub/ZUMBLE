use tokio::sync::RwLock;

use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::state::{ServerState, ServerStateRef};
use std::sync::Arc;
use std::time::Instant;

pub async fn clean_loop(state: ServerStateRef) {
    loop {
        tracing::trace!("cleaning clients");

        match clean_run(state.clone()).await {
            Ok(_) => (),
            Err(e) => {
                tracing::error!("error in clean loop: {}", e);
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn clean_run(state: ServerStateRef) -> Result<(), MumbleError> {
    let mut client_to_delete = Vec::new();
    let mut client_to_disconnect = Vec::new();

    {
        for client in state.clients.iter() {
            if client.publisher.is_closed() {
                client_to_disconnect.push(client.clone());

                continue;
            }

            let now = Instant::now();

            let duration = { now.duration_since(*client.last_ping.read().await) };

            if duration.as_secs() > 60 {
                client_to_delete.push(client.clone());
            }
        }
    }

    for client in client_to_delete {
        {
            let username = { client.authenticate.get_username().to_string() };

            match client.publisher.try_send(ClientMessage::Disconnect) {
                Ok(_) => (),
                Err(err) => {
                    tracing::error!("error sending disconnect signal to {}: {}", username, err);
                }
            }
        };
    }

    for client in client_to_disconnect {
        let (user_id, channel_id) = { state.disconnect(client).await? };

        crate::metrics::CLIENTS_TOTAL.dec();

        {
            state.remove_client(user_id, channel_id).await?;
        }
    }

    Ok(())
}
