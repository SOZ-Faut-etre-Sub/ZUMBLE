use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::state::{ServerState, ServerStateRef};
use std::time::Instant;

pub async fn clean_loop(state: ServerStateRef) {
    loop {
        tracing::trace!("cleaning clients");

        match clean_run(&state).await {
            Ok(_) => (),
            Err(e) => {
                tracing::error!("error in clean loop: {}", e);
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn clean_run(state: &ServerState) -> Result<(), MumbleError> {
    let mut client_to_delete = Vec::new();

    state.clients.retain(|_, client| {
        // if we don't have a publisher then we just want to remove the client
        if client.publisher.is_closed() {
            return false;
        }

        let now = Instant::now();

        let duration = now.duration_since(client.last_ping.load());

        if duration.as_secs() > 30 {
            client_to_delete.push(client.clone());
        }

        true
    });

    for client in client_to_delete {
        let username = { client.authenticate.get_username().to_string() };

        match client.publisher.try_send(ClientMessage::Disconnect) {
            Ok(_) => (),
            Err(err) => {
                tracing::error!("error sending disconnect signal to {}: {}", username, err);
            }
        }
    }

    Ok(())
}
