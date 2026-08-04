use crate::error::{AppError, AppResult};
use crate::models::{CreateSourceRequest, Source, UpdateSourceRequest};
use crate::seeds::get_or_create_folder;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::patch;
use axum::{Json, Router};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/sources", axum::routing::get(list_sources).post(create_source))
        .route("/api/sources/:id", patch(update_source).delete(delete_source))
}

const SOURCE_SELECT: &str = "
    SELECT s.id, s.name, s.rss_url, s.folder_id, f.name,
           s.active, s.custom,
           (SELECT COUNT(*) FROM articles a WHERE a.source_id = s.id) as article_count,
           s.last_fetch, s.last_error
    FROM sources s
    LEFT JOIN folders f ON f.id = s.folder_id";

fn row_to_source(row: &rusqlite::Row) -> rusqlite::Result<Source> {
    Ok(Source {
        id: row.get(0)?,
        name: row.get(1)?,
        rss_url: row.get(2)?,
        folder_id: row.get(3)?,
        folder_name: row.get(4)?,
        active: row.get::<_, i64>(5)? != 0,
        custom: row.get::<_, i64>(6)? != 0,
        article_count: row.get(7)?,
        last_fetch: row.get(8)?,
        last_error: row.get(9)?,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct SourceListQuery {
    #[serde(rename = "folderId")]
    folder_id: Option<i64>,
}

async fn list_sources(
    State(state): State<AppState>,
    Query(q): Query<SourceListQuery>,
) -> AppResult<Json<Vec<Source>>> {
    let conn = state.db.get()?;
    let sources = if let Some(folder_id) = q.folder_id {
        let mut stmt = conn.prepare(&format!(
            "{SOURCE_SELECT} WHERE s.folder_id = ?1 ORDER BY s.name"
        ))?;
        let rows = stmt
            .query_map([folder_id], row_to_source)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    } else {
        let mut stmt = conn.prepare(&format!("{SOURCE_SELECT} ORDER BY s.name"))?;
        let rows = stmt.query_map([], row_to_source)?.collect::<Result<Vec<_>, _>>()?;
        rows
    };
    Ok(Json(sources))
}

async fn create_source(
    State(state): State<AppState>,
    Json(body): Json<CreateSourceRequest>,
) -> AppResult<Json<Source>> {
    let name = body.name.trim();
    let rss_url = body.rss_url.trim();
    if name.is_empty() || rss_url.is_empty() {
        return Err(AppError::BadRequest("name and rssUrl are required".into()));
    }

    let mut conn = state.db.get()?;
    let tx = conn.transaction()?;

    let folder_id = if let Some(id) = body.folder_id {
        // Validar que la carpeta exista; de lo contrario el INSERT
        // dispararía un error de foreign key que el `map_err` de abajo
        // confundiría con "URL duplicada".
        let folder_exists: bool = tx
            .query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(true))
            .unwrap_or(false);
        if !folder_exists {
            return Err(AppError::BadRequest(format!("Folder {id} does not exist")));
        }
        Some(id)
    } else if let Some(folder_name) = body.folder_name.as_deref().filter(|s| !s.trim().is_empty())
    {
        Some(get_or_create_folder(&tx, folder_name)?)
    } else {
        None
    };

    tx.execute(
        "INSERT INTO sources (name, rss_url, folder_id, active, custom) VALUES (?1, ?2, ?3, 1, 1)",
        rusqlite::params![name, rss_url, folder_id],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            AppError::Conflict("A source with that RSS URL already exists".into())
        }
        other => AppError::from(other),
    })?;

    let id = tx.last_insert_rowid();
    let source = tx.query_row(
        &format!("{SOURCE_SELECT} WHERE s.id = ?1"),
        [id],
        row_to_source,
    )?;
    tx.commit()?;
    Ok(Json(source))
}

async fn update_source(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateSourceRequest>,
) -> AppResult<Json<Source>> {
    let mut conn = state.db.get()?;

    // Verificar que la fuente exista antes de tocar nada, para devolver
    // un 404 claro en vez de aplicar UPDATEs que afectan 0 filas.
    let exists: bool = conn
        .query_row("SELECT 1 FROM sources WHERE id = ?1", [id], |_| Ok(true))
        .unwrap_or(false);
    if !exists {
        return Err(AppError::NotFound("Source not found".into()));
    }

    let tx = conn.transaction()?;

    if let Some(name) = &body.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest("name cannot be empty".into()));
        }
        tx.execute("UPDATE sources SET name = ?1 WHERE id = ?2", rusqlite::params![name, id])?;
    }
    if let Some(active) = body.active {
        tx.execute(
            "UPDATE sources SET active = ?1 WHERE id = ?2",
            rusqlite::params![active as i64, id],
        )?;
    }
    if let Some(folder_id) = body.folder_id {
        let folder_id: Option<i64> = if folder_id == 0 { None } else { Some(folder_id) };
        // Validar que la carpeta destino exista (si no es NULL), para dar
        // un 400 legible en vez de un error de foreign key genérico.
        if let Some(fid) = folder_id {
            let folder_exists: bool = tx
                .query_row("SELECT 1 FROM folders WHERE id = ?1", [fid], |_| Ok(true))
                .unwrap_or(false);
            if !folder_exists {
                return Err(AppError::BadRequest(format!("Folder {fid} does not exist")));
            }
        }
        tx.execute(
            "UPDATE sources SET folder_id = ?1 WHERE id = ?2",
            rusqlite::params![folder_id, id],
        )?;
    }

    let source = tx.query_row(&format!("{SOURCE_SELECT} WHERE s.id = ?1"), [id], row_to_source)?;
    tx.commit()?;
    Ok(Json(source))
}

async fn delete_source(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.db.get()?;
    let changed = conn.execute("DELETE FROM sources WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(AppError::NotFound("Source not found".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}
