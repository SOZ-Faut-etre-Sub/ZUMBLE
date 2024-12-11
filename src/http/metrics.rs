use axum::http::StatusCode;
use prometheus::{Encoder, TextEncoder};

// #[actix_web::get("/metrics")]
pub async fn get_metrics() -> Result<Vec<u8>, StatusCode> {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];

    match encoder.encode(&prometheus::gather(), &mut buffer) {
        Ok(_) => Ok(buffer),
        Err(err) => {
            tracing::error!("error encoding metrics: {}", err);

            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
