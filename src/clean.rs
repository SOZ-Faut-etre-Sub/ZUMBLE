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

        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn clean_run(state: Arc<RwLock<ServerState>>) -> Result<(), MumbleError> {
    let mut client_to_delete = Vec::new();

    {
        for client in state.read_err().await?.clients.values() {
            let now = Instant::now();

            let duration = { now.duration_since(*client.read_err().await?.last_ping.read_err().await?) };

            if duration.as_secs() > 60 {
                client_to_delete.push(client.clone());
            }
        }
    }

    for client in client_to_delete {
        {
            match client.read_err().await?.publisher.try_send(ClientMessage::Disconnect) {
                Ok(_) => {}
                Err(err) => {
                    tracing::error!("error sending disconnect signal: {}", err);
                }
            }
        };
    }

    Ok(())
}
