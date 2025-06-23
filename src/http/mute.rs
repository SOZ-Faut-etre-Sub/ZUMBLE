use crate::error::MumbleError;
use crate::sync::RwLock;
use crate::ServerState;
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct Mute {
    mute: bool,
    user: String,
}

#[derive(Serialize, Deserialize)]
pub struct MuteAll {
    mute: bool,
}

#[actix_web::post("/mute")]
pub async fn post_mute(mute: web::Json<Mute>, state: web::Data<Arc<RwLock<ServerState>>>) -> Result<HttpResponse, MumbleError> {
    let client = { state.read_err().await?.get_client_by_name(mute.user.as_str()).await? };

    Ok(match client {
        Some(client) => {
            client.write_err().await?.mute(mute.mute);

            HttpResponse::Ok().finish()
        }
        None => HttpResponse::NotFound().finish(),
    })
}

#[actix_web::post("/mute_all")]
pub async fn post_mute_all(mute_all: web::Json<MuteAll>, state: web::Data<Arc<RwLock<ServerState>>>) -> Result<HttpResponse, MumbleError> {
    state.read_err().await?.mute_all.store(mute_all.mute, std::sync::atomic::Ordering::Relaxed);

    Ok(HttpResponse::Ok().finish())
}

#[actix_web::get("/mute/{user}")]
pub async fn get_mute(user: web::Path<String>, state: web::Data<Arc<RwLock<ServerState>>>) -> Result<HttpResponse, MumbleError> {
    let username = user.into_inner();
    let client = { state.read_err().await?.get_client_by_name(username.as_str()).await? };

    Ok(match client {
        Some(client) => {
            let mute = Mute {
                mute: { client.read_err().await?.mute },
                user: username,
            };

            HttpResponse::Ok().json(&mute)
        }
        None => HttpResponse::NotFound().finish(),
    })
}
