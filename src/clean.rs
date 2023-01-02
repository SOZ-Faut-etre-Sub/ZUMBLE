use crate::state::ServerState;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

pub async fn clean_loop(state: Arc<RwLock<ServerState>>) {
    loop {
        let mut client_to_delete = Vec::new();

        {
            for client in state.read().await.clients.values() {
                let now = Instant::now();

                let duration = { now.duration_since(client.read().await.last_ping.read().await.clone()) };

                if duration.as_secs() > 60 {
                    client_to_delete.push(client.clone());
                }
            }
        }

        for client in client_to_delete {
            {
                match client.read().await.publisher_disconnect.send(true).await {
                    Ok(_) => {}
                    Err(err) => {
                        tracing::error!("error sending disconnect signal: {}", err);
                    }
                }
            };
        }

        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}
