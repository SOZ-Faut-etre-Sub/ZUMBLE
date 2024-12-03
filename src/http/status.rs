use crate::error::MumbleError;
use crate::state::ServerStateRef;
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Instant;

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
pub async fn get_status(state: web::Data<ServerStateRef>) -> Result<HttpResponse, MumbleError> {
    let mut clients = HashMap::new();
    for client in state.clients.iter() {
        let session = client.session_id;
        let channel_id = { client.channel_id.load(Ordering::Relaxed) };
        let channel = { state.channels.get(&channel_id) };
        let channel_name = {
            if let Some(channel) = channel {
                Some(channel.name.clone())
            } else {
                None
            }
        };

        {
            let crypt_state = client.crypt_state.read().await;

            let mut mumble_client = MumbleClient {
                name: client.authenticate.get_username().to_string(),
                session_id: client.session_id,
                channel: channel_name,
                mute: client.is_muted(),
                good: crypt_state.good,
                late: crypt_state.late,
                lost: crypt_state.lost,
                resync: crypt_state.resync,
                last_good_duration: Instant::now().duration_since(crypt_state.last_good).as_millis(),
                targets: Vec::new(),
            };

            for target in &client.targets {
                let mumble_target = {
                    MumbleTarget {
                        sessions: target.sessions.iter().map(|v| *v.key()).collect(),
                        channels: target.channels.iter().map(|v| *v.key()).collect(),
                    }
                };

                mumble_client.targets.push(mumble_target);
            }

            clients.insert(session, mumble_client);
        }
    }

    Ok(HttpResponse::Ok().json(&clients))
}
