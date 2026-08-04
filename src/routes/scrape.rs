use crate::error::AppResult;
use crate::models::ScrapeStatus;
use crate::scheduler::run_scrape_once;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/scrape", post(trigger_scrape))
        .route("/api/scrape/status", get(scrape_status))
}

/// Dispara un scrape en background y responde de inmediato (igual que
/// el original: el cliente puede consultar `/api/scrape/status` para ver
/// cuándo termina).
async fn trigger_scrape(
    State(state): State<AppState>,
) -> AppResult<Json<serde_json::Value>> {
    tokio::spawn(run_scrape_once(state));
    Ok(Json(serde_json::json!({ "success": true, "message": "Scraping started" })))
}

async fn scrape_status(State(state): State<AppState>) -> Json<ScrapeStatus> {
    let status = state.scrape_state.read().await;
    Json(ScrapeStatus {
        scraping: status.scraping,
        started_at: status.started_at.clone(),
        last_finished_at: status.last_finished_at.clone(),
        last_total_new: status.last_total_new,
        last_errors: status.last_errors.clone(),
    })
}
