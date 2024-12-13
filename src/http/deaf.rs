use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::AppStateRef;

#[derive(Serialize, Deserialize)]
pub struct Deaf {
    deaf: bool,
    user: String,
}

// #[actix_web::post("/deaf")]
pub async fn post_deaf(State(state): State<AppStateRef>, Json(deaf): Json<Deaf>) -> StatusCode {
    let client = state.server.get_client_by_name(deaf.user.as_str()).await;

    match client {
        Some(client) => {
            client.set_deaf(deaf.deaf);

            StatusCode::OK
        }
        None => StatusCode::NOT_FOUND,
    }
}

// #[actix_web::get("/deaf/{user}")]
pub async fn get_deaf(Path(username): Path<String>, State(state): State<AppStateRef>) -> Result<Json<Deaf>, StatusCode> {
    println!("??");
    if let Some(client) = state.server.get_client_by_name(username.as_str()).await {
        let deaf = Deaf {
            deaf: client.is_deaf(),
            user: username,
        };

        return Ok(Json(deaf));
    }

    Err(StatusCode::NOT_FOUND)
}
