use crate::{error::MumbleError, state::ServerStateRef};
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Deaf {
    deaf: bool,
    user: String,
}

#[actix_web::post("/deaf")]
pub async fn post_deaf(deaf: web::Json<Deaf>, state: web::Data<ServerStateRef>) -> Result<HttpResponse, MumbleError> {
    let client = { state.get_client_by_name(deaf.user.as_str()) };

    Ok(match client {
        Some(client) => {
            client.set_deaf(deaf.deaf);

            HttpResponse::Ok().finish()
        }
        None => HttpResponse::NotFound().finish(),
    })
}

#[actix_web::get("/deaf/{user}")]
pub async fn get_deaf(user: web::Path<String>, state: web::Data<ServerStateRef>) -> Result<HttpResponse, MumbleError> {
    let username = user.into_inner();
    let client = { state.get_client_by_name(username.as_str()) };

    let var_name = match client {
        Some(client) => {
            let deaf = Deaf {
                deaf: client.is_deaf(),
                user: username,
            };

            HttpResponse::Ok().json(&deaf)
        }
        None => HttpResponse::NotFound().finish(),
    };
    Ok(var_name)
}
