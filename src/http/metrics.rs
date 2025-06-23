use actix_web::HttpResponse;
use prometheus::{Encoder, TextEncoder};

#[actix_web::get("/metrics")]
pub async fn get_metrics() -> HttpResponse {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];

    match encoder.encode(&prometheus::gather(), &mut buffer) {
        Ok(_) => HttpResponse::Ok().body(buffer),
        Err(err) => {
            tracing::error!("error encoding metrics: {}", err);

            HttpResponse::InternalServerError().finish()
        }
    }
}
