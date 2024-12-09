use crate::error::MumbleError;
use crate::message::ClientMessage;
use crate::state::{ServerState, ServerStateRef};
use std::sync::Arc;
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

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

async fn clean_run(state: &ServerState) -> Result<(), MumbleError> {
    let mut client_to_disconnect = Vec::new();
    let mut clients_to_remove = Vec::new();
    let mut clients_to_reset_crypt = Vec::new();

    let mut iter = state.clients_by_socket.first_entry();
    while let Some(client) = iter {
        if client.publisher.is_closed() {
            clients_to_remove.push(client.session_id);
        }

        let now = Instant::now();

        let duration = now.duration_since(client.last_ping.load());

        if duration.as_secs() > 30 {
            client_to_disconnect.push(client.clone());
        }

        let last_good = { client.crypt_state.lock().last_good };

        if now.duration_since(last_good).as_millis() > 8000 {
            clients_to_reset_crypt.push(client.clone())
        }

        iter = client.next();
    }

    for client in clients_to_reset_crypt {
        let log_name = Arc::clone(&client);
        let session_id = client.session_id;
        if let Err(e) = state.reset_client_crypt(client).await {
            tracing::error!("failed to send crypt setup for {}: {:?}", e, session_id);
        } else {
            tracing::info!("Requesting {} crypt be reset", log_name);
        }
    }

    for client in client_to_disconnect {

        match client.publisher.try_send(ClientMessage::Disconnect) {
            Ok(_) => (),
            Err(err) => {
                tracing::error!("error sending disconnect signal to {}: {}", client, err);
            }
        }
    }

    for session_id in clients_to_remove {
        state.disconnect(session_id);
    }

    Ok(())
}
