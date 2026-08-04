use crate::error::{AppError, AppResult};
use crate::models::{Article, ArticleQuery, UpdateArticleRequest};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::post;
use axum::{Json, Router};
use rusqlite::types::Value as SqlValue;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/articles", axum::routing::get(list_articles))
        .route("/api/articles/mark-all-read", post(mark_all_read))
        .route(
            "/api/articles/:id",
            axum::routing::get(get_article).patch(update_article),
        )
}

const ARTICLE_SELECT: &str = "
    SELECT a.id, a.title, a.summary, a.source_id, s.name, s.folder_id, f.name,
           a.severity, a.published_at, a.read_time, a.url, a.image_url, a.is_read, a.is_starred
    FROM articles a
    JOIN sources s ON s.id = a.source_id
    LEFT JOIN folders f ON f.id = s.folder_id";

fn row_to_article(row: &rusqlite::Row) -> rusqlite::Result<Article> {
    Ok(Article {
        id: row.get(0)?,
        title: row.get(1)?,
        summary: row.get(2)?,
        source_id: row.get(3)?,
        source_name: row.get(4)?,
        folder_id: row.get(5)?,
        folder_name: row.get(6)?,
        severity: row.get(7)?,
        published_at: row.get(8)?,
        read_time: row.get(9)?,
        url: row.get(10)?,
        image_url: row.get(11)?,
        is_read: row.get::<_, i64>(12)? != 0,
        is_starred: row.get::<_, i64>(13)? != 0,
    })
}

async fn list_articles(
    State(state): State<AppState>,
    Query(q): Query<ArticleQuery>,
) -> AppResult<Json<Vec<Article>>> {
    let mut sql = format!("{ARTICLE_SELECT} WHERE 1=1");
    let mut params: Vec<SqlValue> = Vec::new();

    if let Some(folder_id) = q.folder_id {
        sql.push_str(" AND s.folder_id = ?");
        params.push(SqlValue::Integer(folder_id));
    }
    if let Some(source_id) = q.source_id {
        sql.push_str(" AND a.source_id = ?");
        params.push(SqlValue::Integer(source_id));
    }
    if let Some(search) = &q.search {
        let search = search.trim();
        if !search.is_empty() {
            sql.push_str(" AND (a.title LIKE ? OR a.summary LIKE ?)");
            let pattern = format!("%{search}%");
            params.push(SqlValue::Text(pattern.clone()));
            params.push(SqlValue::Text(pattern));
        }
    }
    if q.unread_only.unwrap_or(false) {
        sql.push_str(" AND a.is_read = 0");
    }
    if q.starred_only.unwrap_or(false) {
        sql.push_str(" AND a.is_starred = 1");
    }

    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    sql.push_str(" ORDER BY a.published_at DESC LIMIT ? OFFSET ?");
    params.push(SqlValue::Integer(limit));
    params.push(SqlValue::Integer(offset));

    let conn = state.db.get()?;
    let mut stmt = conn.prepare(&sql)?;
    let articles = stmt
        .query_map(rusqlite::params_from_iter(params), row_to_article)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(articles))
}

async fn get_article(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Article>> {
    let conn = state.db.get()?;
    let article = conn
        .query_row(&format!("{ARTICLE_SELECT} WHERE a.id = ?1"), [id], row_to_article)
        .map_err(|_| AppError::NotFound("Article not found".into()))?;
    Ok(Json(article))
}

async fn update_article(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateArticleRequest>,
) -> AppResult<Json<Article>> {
    let conn = state.db.get()?;

    if let Some(is_read) = body.is_read {
        conn.execute(
            "UPDATE articles SET is_read = ?1 WHERE id = ?2",
            rusqlite::params![is_read as i64, id],
        )?;
    }
    if let Some(is_starred) = body.is_starred {
        conn.execute(
            "UPDATE articles SET is_starred = ?1 WHERE id = ?2",
            rusqlite::params![is_starred as i64, id],
        )?;
    }

    let article = conn
        .query_row(&format!("{ARTICLE_SELECT} WHERE a.id = ?1"), [id], row_to_article)
        .map_err(|_| AppError::NotFound("Article not found".into()))?;
    Ok(Json(article))
}

async fn mark_all_read(
    State(state): State<AppState>,
    Query(q): Query<ArticleQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let conn = state.db.get()?;
    let updated = if let Some(folder_id) = q.folder_id {
        conn.execute(
            "UPDATE articles SET is_read = 1
             WHERE is_read = 0 AND source_id IN (SELECT id FROM sources WHERE folder_id = ?1)",
            [folder_id],
        )?
    } else if let Some(source_id) = q.source_id {
        conn.execute(
            "UPDATE articles SET is_read = 1 WHERE is_read = 0 AND source_id = ?1",
            [source_id],
        )?
    } else {
        conn.execute("UPDATE articles SET is_read = 1 WHERE is_read = 0", [])?
    };
    Ok(Json(serde_json::json!({ "success": true, "updated": updated })))
}
