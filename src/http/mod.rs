mod deaf;
mod metrics;
mod mute;
mod status;

use crate::state::ServerStateRef;
use crate::ServerState;
use actix_server::Server;
use actix_web::middleware::Condition;
use actix_web::{middleware, web, App, HttpServer};
use actix_web_httpauth::{extractors::AuthenticationError, headers::www_authenticate::basic::Basic, middleware::HttpAuthentication};
use std::sync::Arc;
use tokio::sync::RwLock;

pub fn create_http_server(
    listen: String,
    tls_config: rustls::ServerConfig,
    use_tls: bool,
    state: ServerStateRef,
    user: String,
    password: Option<String>,
    log_requests: bool,
) -> Option<Server> {
    let mut server = HttpServer::new(move || {
        let user = user.clone();
        let password = password.clone();

        let auth = HttpAuthentication::basic(move |req, credentials| {
            let user = user.clone();
            let password = password.clone();

            async move {
                let password = password.clone();

                if password.is_none() {
                    return Err((AuthenticationError::new(Basic::with_realm("Restricted area")).into(), req));
                }

                let user = user.clone();

                if credentials.user_id() == user.as_str() && credentials.password() == Some(password.unwrap().as_str()) {
                    Ok(req)
                } else {
                    Err((AuthenticationError::new(Basic::with_realm("Restricted area")).into(), req))
                }
            }
        });

        let mut logger = middleware::Logger::default();
        logger = logger.exclude("/metrics").exclude("/status").log_target("log_http");

        App::new()
            .app_data(web::Data::new(state.clone()))
            .wrap(auth)
            .wrap(Condition::new(log_requests, logger))
            .service(metrics::get_metrics)
            .service(mute::get_mute)
            .service(mute::post_mute)
            .service(deaf::get_deaf)
            .service(deaf::post_deaf)
            .service(status::get_status)
    });

    server = if use_tls {
        server
            .bind_rustls_0_23(listen, tls_config)
            .map_err(|e| {
                tracing::error!("bind error: {}", e);
            })
            .ok()?
    } else {
        server
            .bind(listen)
            .map_err(|e| {
                tracing::error!("bind error: {}", e);
            })
            .ok()?
    };

    Some(server.run())
}
