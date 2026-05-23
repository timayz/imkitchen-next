mod assets;
mod auth;

use std::time::Duration;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    middleware::map_response,
    response::IntoResponse,
    routing::{get, post},
};
use imkitchen_common::minify_response;
use serde::Deserialize;
use sqlx::SqlitePool;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    limit::RequestBodyLimitLayer,
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

pub use auth::Admin;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub port: u16,
    pub timeout_secs: u64,
    pub body_limit_bytes: usize,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub read_pool: SqlitePool,
    pub write_pool: SqlitePool,
}

pub fn router(state: AppState) -> Router {
    let sensitive = [header::AUTHORIZATION, header::COOKIE];
    let timeout = Duration::from_secs(state.config.timeout_secs);
    let body_limit = state.config.body_limit_bytes;

    Router::new()
        .route("/", get(index))
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route("/logout", post(auth::logout))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .fallback_service(assets::AssetsService::new())
        .with_state(state)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            timeout,
        ))
        .layer(RequestBodyLimitLayer::new(body_limit))
        .layer(SetSensitiveResponseHeadersLayer::new(sensitive.clone()))
        .layer(map_response(minify_response))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(SetSensitiveRequestHeadersLayer::new(sensitive))
        .layer(CatchPanicLayer::new())
}

async fn index(_admin: Admin) -> impl IntoResponse {
    axum::response::Html(
        "<p>imkitchen admin — signed in as <strong>admin</strong></p>\
         <form method=\"post\" action=\"/logout\"><button>Log out</button></form>",
    )
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.read_pool).await {
        Ok(_) => (StatusCode::OK, "ready"),
        Err(err) => {
            tracing::warn!(error = %err, "readiness check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "not ready")
        }
    }
}
