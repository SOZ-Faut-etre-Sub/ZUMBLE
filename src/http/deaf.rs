use crate::error::MumbleError;
use crate::sync::RwLock;
use crate::ServerState;
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct Deaf {
    deaf: bool,
    user: String,
}

#[actix_web::post("/deaf")]
pub async fn post_deaf(deaf: web::Json<Deaf>, state: web::Data<Arc<RwLock<ServerState>>>) -> Result<HttpResponse, MumbleError> {
    let client = { state.read_err().await?.get_client_by_name(deaf.user.as_str()).await? };

    Ok(match client {
        Some(client) => {
            client.write_err().await?.deaf(deaf.deaf);

            HttpResponse::Ok().finish()
        }
        None => HttpResponse::NotFound().finish(),
    })
}

#[actix_web::get("/deaf/{user}")]
pub async fn get_deaf(user: web::Path<String>, state: web::Data<Arc<RwLock<ServerState>>>) -> Result<HttpResponse, MumbleError> {
    let username = user.into_inner();
    let client = { state.read_err().await?.get_client_by_name(username.as_str()).await? };

    Ok(match client {
        Some(client) => {
            let deaf = Deaf {
                deaf: { client.read_err().await?.deaf },
                user: username,
            };

            HttpResponse::Ok().json(&deaf)
        }
        None => HttpResponse::NotFound().finish(),
    })
}
