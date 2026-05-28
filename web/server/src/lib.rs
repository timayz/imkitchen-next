mod assets;
mod auth;
mod recipes;
mod recipes_create;
mod recipes_edit;
mod recipes_import;

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    middleware::map_response,
    response::IntoResponse,
    routing::{get, post},
};
use imkitchen_common::minify_response;
use imkitchen_recipes::import::RecipeParser;
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

pub use auth::{Chef, Premium, Role, User};

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
    /// Evento executor pointed at the write pool. The read-side projection
    /// queries hit `read_pool` directly via the projection helpers.
    pub evento: evento::Sqlite,
    /// Recipe parser used by the multipart upload path (`/recipes/import`
    /// POST). The same instance is shared with the import saga so file
    /// uploads and post-confirmation `materialize` calls go through the
    /// same parser implementation.
    pub recipe_parser: Arc<dyn RecipeParser>,
}

pub fn router(state: AppState) -> Router {
    let sensitive = [header::AUTHORIZATION, header::COOKIE];
    let timeout = Duration::from_secs(state.config.timeout_secs);
    let body_limit = state.config.body_limit_bytes;

    Router::new()
        .route("/", get(index))
        .route("/recipes", get(recipes::recipes_index))
        .route(
            "/recipes/new",
            get(recipes_create::create_form).post(recipes_create::create_submit),
        )
        .route(
            "/recipes/new/ingredient-row",
            get(recipes_create::ingredient_row_fragment),
        )
        .route(
            "/recipes/new/step-row",
            get(recipes_create::step_row_fragment),
        )
        .route(
            "/recipes/import",
            get(recipes_import::import_page).post(recipes_import::import_submit),
        )
        .route("/recipes/{id}", get(recipes::recipe_detail))
        .route("/recipes/{id}/edit", get(recipes_edit::edit_page))
        .route(
            "/recipes/{id}/title",
            post(recipes_edit::rename_section),
        )
        .route(
            "/recipes/{id}/categorization",
            post(recipes_edit::recategorize_section),
        )
        .route("/recipes/{id}/timing", post(recipes_edit::retime_section))
        .route(
            "/recipes/{id}/description",
            post(recipes_edit::redescribe_section),
        )
        .route("/recipes/{id}/tags", post(recipes_edit::retag_section))
        .route(
            "/recipes/{id}/ingredients",
            post(recipes_edit::ingredients_section),
        )
        .route("/recipes/{id}/steps", post(recipes_edit::steps_section))
        .route("/recipes/{id}/delete", post(recipes_edit::delete_section))
        .route("/share-recipe", get(share_recipe))
        .route("/premium", get(premium_only))
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

async fn index(user: User) -> impl IntoResponse {
    axum::response::Html(format!(
        "<p>imkitchen web — signed in as <strong>{}</strong></p>\
         <form method=\"post\" action=\"/logout\"><button>Log out</button></form>",
        user.role.as_str()
    ))
}

async fn share_recipe(_chef: Chef) -> impl IntoResponse {
    "chef-only: share a recipe"
}

async fn premium_only(_premium: Premium) -> impl IntoResponse {
    "premium-only content"
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
