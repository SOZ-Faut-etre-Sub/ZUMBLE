use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use axum_auth::AuthBasic;

use super::AppStateRef;

pub async fn auth_basic(
    State(auth_state): State<AppStateRef>,
    // run the `HeaderMap` extractor
    AuthBasic((id, password)): AuthBasic,
    // you can also add more extractors here but the last
    // extractor must implement `FromRequest` which
    // `Request` does
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if auth_state.auth.password.is_none() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if id == auth_state.auth.username && password == auth_state.auth.password {
        let response = next.run(request).await;
        return Ok(response);
    }
    Err(StatusCode::UNAUTHORIZED)
}
