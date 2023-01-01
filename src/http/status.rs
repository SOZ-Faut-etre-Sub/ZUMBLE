use crate::ServerState;
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

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
    pub sessions: HashSet<u32>,
    pub channels: HashSet<u32>,
}

#[actix_web::get("/status")]
pub async fn get_status(state: web::Data<Arc<RwLock<ServerState>>>) -> HttpResponse {
    let mut clients = HashMap::new();
    let sessions = { state.read().await.clients.keys().cloned().collect::<Vec<u32>>() };

    for session in sessions {
        let client = { state.read().await.clients.get(&session).cloned() };

        if let Some(client) = client {
            let channel_id = { client.read().await.channel_id };
            let channel = { state.read().await.channels.get(&channel_id).cloned() };
            let channel_name = {
                if let Some(channel) = channel {
                    Some(channel.read().await.name.clone())
                } else {
                    None
                }
            };

            {
                let client_read = client.read().await;
                let crypt_state = client_read.crypt_state.read().await;

                let mut mumble_client = MumbleClient {
                    name: client_read.authenticate.get_username().to_string(),
                    session_id: client_read.session_id,
                    channel: channel_name,
                    mute: client_read.mute,
                    good: crypt_state.good,
                    late: crypt_state.late,
                    lost: crypt_state.lost,
                    resync: crypt_state.resync,
                    last_good_duration: Instant::now().duration_since(crypt_state.last_good).as_millis(),
                    targets: Vec::new(),
                };

                for target in &client_read.targets {
                    let mumble_target = {
                        let target_read = target.read().await;

                        MumbleTarget {
                            sessions: target_read.sessions.clone(),
                            channels: target_read.channels.clone(),
                        }
                    };

                    mumble_client.targets.push(mumble_target);
                }

                clients.insert(session, mumble_client);
            }
        }
    }

    HttpResponse::Ok().json(&clients)
}
