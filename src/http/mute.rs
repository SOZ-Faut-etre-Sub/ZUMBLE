use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::AppStateRef;

#[derive(Serialize, Deserialize)]
pub struct Mute {
    mute: bool,
    user: String,
}

pub async fn post_mute(State(state): State<AppStateRef>, Json(mute): Json<Mute>) -> StatusCode {
    if let Some(client) = state.server.get_client_by_name(mute.user.as_str()).await {
        client.set_mute(mute.mute);

        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

// #[actix_web::get("/mute/{user}")]
pub async fn get_mute(Path(username): Path<String>, State(state): State<AppStateRef>) -> Result<Json<Mute>, StatusCode> {
    if let Some(client) = state.server.get_client_by_name(username.as_str()).await {
        let mute = Mute {
            mute: client.is_muted(),
            user: username,
        };

        return Ok(Json(mute));
    }

    Err(StatusCode::NOT_FOUND)
}
