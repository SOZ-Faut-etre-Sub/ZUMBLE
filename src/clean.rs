use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::state::ServerState;
use crate::sync::RwLock;
use std::sync::Arc;
use std::time::Instant;

pub async fn clean_loop(state: Arc<RwLock<ServerState>>) {
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

async fn clean_run(state: Arc<RwLock<ServerState>>) -> Result<(), MumbleError> {
    let mut client_to_delete = Vec::new();
    let mut client_to_disconnect = Vec::new();

    {
        for client in state.read_err().await?.clients.values() {
            if client.read_err().await?.publisher.is_closed() {
                client_to_disconnect.push(client.clone());

                continue;
            }

            let now = Instant::now();

            let duration = { now.duration_since(*client.read_err().await?.last_ping.read_err().await?) };

            if duration.as_secs() > 60 {
                client_to_delete.push(client.clone());
            }
        }
    }

    for client in client_to_delete {
        {
            let username = { client.read_err().await?.authenticate.get_username().to_string() };

            match client.read_err().await?.publisher.try_send(ClientMessage::Disconnect) {
                Ok(_) => (),
                Err(err) => {
                    tracing::error!("error sending disconnect signal to {}: {}", username, err);
                }
            }
        };
    }

    for client in client_to_disconnect {
        let (user_id, channel_id) = { state.write_err().await?.disconnect(client).await? };

        crate::metrics::CLIENTS_TOTAL.dec();

        {
            state.read_err().await?.remove_client(user_id, channel_id).await?;
        }
    }

    Ok(())
}
