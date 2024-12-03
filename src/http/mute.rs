use crate::{error::MumbleError, state::ServerStateRef};
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Mute {
    mute: bool,
    user: String,
}

#[actix_web::post("/mute")]
pub async fn post_mute(mute: web::Json<Mute>, state: web::Data<ServerStateRef>) -> Result<HttpResponse, MumbleError> {
    let client = { state.get_client_by_name(mute.user.as_str()) };

    Ok(match client {
        Some(client) => {
            client.mute(mute.mute);

            HttpResponse::Ok().finish()
        }
        None => HttpResponse::NotFound().finish(),
    })
}

#[actix_web::get("/mute/{user}")]
pub async fn get_mute(user: web::Path<String>, state: web::Data<ServerStateRef>) -> Result<HttpResponse, MumbleError> {
    let username = user.into_inner();
    let client = { state.get_client_by_name(username.as_str()) };

    Ok(match client {
        Some(client) => {
            let mute = Mute {
                mute: client.is_muted(),
                user: username,
            };

            HttpResponse::Ok().json(&mute)
        }
        None => HttpResponse::NotFound().finish(),
    })
}
