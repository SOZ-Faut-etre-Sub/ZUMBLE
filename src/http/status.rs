use axum::extract::State;
use axum::Json;
use scc::ebr::Guard;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::AppStateRef;

#[derive(Serialize, Deserialize)]
pub struct MumbleClient {
    pub name: String,
    pub session_id: u32,
    pub channel: Option<String>,
    pub mute: bool,
    pub good: u32,
    pub late: u32,
    pub lost: u32,
    pub resync: u32,
    pub last_good_duration: u128,
    pub targets: Vec<MumbleTarget>,
}

#[derive(Serialize, Deserialize)]
pub struct MumbleTarget {
    // TODO: provide the target id in the iteration
    // pub target_id: u32,
    pub sessions: HashSet<u32>,
    pub channels: HashSet<u32>,
}

// #[actix_web::get("/status")]
pub async fn get_status(State(state): State<AppStateRef>) -> Json<HashMap<u32, MumbleClient>> {
    let mut clients = HashMap::new();
    let mut iter = state.server.clients.first_entry_async().await;
    while let Some(client_entry) = iter {
        let client = client_entry.get();
        let session = client.session_id;
        let channel_id = { client.channel_id.load(Ordering::Relaxed) };
        let mut channel_name = None;

        {
            let guard = Guard::new();
            if let Some(channel) = state.server.channels.peek(&channel_id, &guard) {
                channel_name = Some(channel.name.clone())
            }
        }

        {
            let (good, late, lost, resync, last_good) = {
                let crypt = client.crypt_state.lock().await;
                (crypt.good, crypt.late, crypt.lost, crypt.resync, crypt.last_good)
            };

            let mut mumble_client = MumbleClient {
                name: client.get_name().as_ref().clone(),
                session_id: client.session_id,
                channel: channel_name,
                mute: client.is_muted(),
                good,
                late,
                lost,
                resync,
                last_good_duration: Instant::now().duration_since(last_good).as_millis(),
                targets: Vec::new(),
            };

            for target in &client.targets {
                let mut sessions = HashSet::new();
                let mut channels = HashSet::new();

                {
                    let guard = Guard::new();
                    for (session, _) in target.sessions.iter(&guard) {
                        sessions.insert(*session);
                    }
                }

                {
                    let guard = Guard::new();
                    for (channel, _) in target.channels.iter(&guard) {
                        channels.insert(*channel);
                    }
                }
                let mumble_target = { MumbleTarget { sessions, channels } };

                mumble_client.targets.push(mumble_target);
            }

            clients.insert(session, mumble_client);
        }
        iter = client_entry.next_async().await;
    }

    Json(clients)
}
