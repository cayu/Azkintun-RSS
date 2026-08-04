use crate::error::{AppError, AppResult};
use crate::models::{CreateFolderRequest, Folder, UpdateFolderRequest};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/folders", get(list_folders).post(create_folder))
        .route(
            "/api/folders/:id",
            patch(update_folder).delete(delete_folder),
        )
        .route("/api/folders/:id/mark-all-read", post(mark_folder_read))
}

fn row_to_folder(row: &rusqlite::Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        name: row.get(1)?,
        source_count: row.get(2)?,
        unread_count: row.get(3)?,
    })
}

const FOLDER_SELECT: &str = "
    SELECT f.id, f.name,
        (SELECT COUNT(*) FROM sources s WHERE s.folder_id = f.id) as source_count,
        (SELECT COUNT(*) FROM articles a
            JOIN sources s ON s.id = a.source_id
            WHERE s.folder_id = f.id AND a.is_read = 0) as unread_count
    FROM folders f";

async fn list_folders(State(state): State<AppState>) -> AppResult<Json<Vec<Folder>>> {
    let conn = state.db.get()?;
    let mut stmt = conn.prepare(&format!("{FOLDER_SELECT} ORDER BY f.name"))?;
    let folders = stmt
        .query_map([], row_to_folder)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(folders))
}

async fn create_folder(
    State(state): State<AppState>,
    Json(body): Json<CreateFolderRequest>,
) -> AppResult<Json<Folder>> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let conn = state.db.get()?;
    let inserted = conn
        .execute("INSERT INTO folders (name) VALUES (?1)", [name])
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                AppError::Conflict("A folder with that name already exists".into())
            }
            other => AppError::from(other),
        })?;
    let _ = inserted;

    let id = conn.last_insert_rowid();
    let folder = conn.query_row(
        &format!("{FOLDER_SELECT} WHERE f.id = ?1"),
        [id],
        row_to_folder,
    )?;
    Ok(Json(folder))
}

async fn update_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateFolderRequest>,
) -> AppResult<Json<Folder>> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let conn = state.db.get()?;
    let changed = conn
        .execute("UPDATE folders SET name = ?1 WHERE id = ?2", rusqlite::params![name, id])
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                AppError::Conflict("A folder with that name already exists".into())
            }
            other => AppError::from(other),
        })?;

    if changed == 0 {
        return Err(AppError::NotFound("Folder not found".into()));
    }

    let folder = conn.query_row(
        &format!("{FOLDER_SELECT} WHERE f.id = ?1"),
        [id],
        row_to_folder,
    )?;
    Ok(Json(folder))
}

/// Borra la carpeta; las fuentes que apuntaban a ella quedan sin carpeta
/// (folder_id = NULL) gracias al `ON DELETE SET NULL` del schema, no se
/// borran sus artículos.
async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.db.get()?;
    let changed = conn.execute("DELETE FROM folders WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(AppError::NotFound("Folder not found".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn mark_folder_read(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.db.get()?;
    let changed = conn.execute(
        "UPDATE articles SET is_read = 1
         WHERE source_id IN (SELECT id FROM sources WHERE folder_id = ?1)",
        [id],
    )?;
    Ok(Json(serde_json::json!({ "success": true, "updated": changed })))
}
