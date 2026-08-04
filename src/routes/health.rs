use crate::error::AppResult;
use crate::models::Stats;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

/// `/api/health`: público, solo para el healthcheck de Docker/monitoreo.
/// No expone datos de la app.
pub fn public_router() -> Router<AppState> {
    Router::new().route("/api/health", get(health))
}

/// `/api/stats`: requiere autenticación (expone datos).
pub fn protected_router() -> Router<AppState> {
    Router::new().route("/api/stats", get(stats))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn stats(State(state): State<AppState>) -> AppResult<Json<Stats>> {
    let conn = state.db.get()?;

    let total_articles: i64 = conn.query_row("SELECT COUNT(*) FROM articles", [], |r| r.get(0))?;
    let total_sources: i64 =
        conn.query_row("SELECT COUNT(*) FROM sources WHERE active = 1", [], |r| r.get(0))?;
    let total_folders: i64 = conn.query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0))?;
    let unread_articles: i64 =
        conn.query_row("SELECT COUNT(*) FROM articles WHERE is_read = 0", [], |r| r.get(0))?;
    let last_fetch: Option<String> =
        conn.query_row("SELECT MAX(fetched_at) FROM fetch_log", [], |r| r.get(0))?;

    Ok(Json(Stats {
        total_articles,
        total_sources,
        total_folders,
        unread_articles,
        last_fetch,
    }))
}
