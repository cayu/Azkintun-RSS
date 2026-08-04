use crate::error::{AppError, AppResult};
use crate::importer::{insert_parsed_feeds, parse_csv, parse_opml};
use crate::models::ImportResult;
use crate::state::AppState;
use axum::extract::{Multipart, State};
use axum::routing::post;
use axum::{Json, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/import/csv", post(import_csv))
        .route("/api/import/opml", post(import_opml))
}

async fn read_upload(mut multipart: Multipart) -> AppResult<Vec<u8>> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("invalid multipart body: {e}")))?
    {
        // Aceptamos cualquier nombre de campo: el frontend puede mandarlo
        // como "file", "csv", "opml", etc. El primer field con bytes gana.
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("error reading upload: {e}")))?;
        if !bytes.is_empty() {
            return Ok(bytes.to_vec());
        }
    }
    Err(AppError::BadRequest("No file was uploaded".into()))
}

/// Importa un CSV (estilo export de Inoreader/Feedly u otro, con
/// columnas detectadas por nombre - ver `importer::parse_csv`).
async fn import_csv(
    State(state): State<AppState>,
    multipart: Multipart,
) -> AppResult<Json<ImportResult>> {
    let bytes = read_upload(multipart).await?;
    let (feeds, errors) = parse_csv(&bytes)?;
    let result = insert_parsed_feeds(&state, feeds, errors)?;
    Ok(Json(result))
}

/// Importa un OPML (el formato estándar real de export/import de
/// Inoreader y de prácticamente cualquier lector RSS).
async fn import_opml(
    State(state): State<AppState>,
    multipart: Multipart,
) -> AppResult<Json<ImportResult>> {
    let bytes = read_upload(multipart).await?;
    let (feeds, errors) = parse_opml(&bytes)?;
    let result = insert_parsed_feeds(&state, feeds, errors)?;
    Ok(Json(result))
}
