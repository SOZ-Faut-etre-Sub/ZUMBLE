mod axum_auth_wrapper;
mod deaf;
mod metrics;
mod mute;
mod status;

use std::sync::Arc;

use axum::{
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};
use axum_auth_wrapper::auth_basic;
use deaf::{get_deaf, post_deaf};
use metrics::get_metrics;
use mute::{get_mute, post_mute};
use status::get_status;

use crate::state::ServerStateRef;

pub struct AuthState {
    username: String,
    password: Option<String>,
}

pub type AppStateRef = Arc<AppState>;

pub struct AppState {
    server: ServerStateRef,
    auth: AuthState,
}

pub fn create_http_server(state: ServerStateRef, username: String, password: Option<String>) -> Option<Router> {
    // if we don't have a password we shouldn't create the HTTP endpoint at all
    password.as_ref()?;
    let app_state = Arc::new(AppState {
        server: state.clone(),
        auth: AuthState { username, password },
    });

    Some(
        Router::new()
            .route("/deaf", post(post_deaf))
            .route("/deaf/:player_id", get(get_deaf))
            .route("/metrics", get(get_metrics))
            .route("/mute", post(post_mute))
            .route("/mute/:player_id", get(get_mute))
            .route("/status", get(get_status))
            .route_layer(from_fn_with_state(app_state.clone(), auth_basic))
            .with_state(app_state),
    )
}
