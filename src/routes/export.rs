//! Exportación de suscripciones en formatos estándar.
//!
//! - OPML 1.0: formato nativo de todo lector RSS, lo aceptan Inoreader,
//!   Feedly, NewsBlur, Miniflux, etc.
//! - CSV: compatible con el importer del propio Azkintun-RSS y con
//!   parsers genéricos.

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::http::header;
use axum::response::Response;
use axum::routing::get;
use axum::{body::Body, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/export/opml", get(export_opml))
        .route("/api/export/csv",  get(export_csv))
}

struct FeedRow {
    name:        String,
    rss_url:     String,
    folder_name: Option<String>,
}

fn load_feeds(state: &AppState) -> AppResult<Vec<FeedRow>> {
    let conn = state.db.get()?;
    let mut stmt = conn.prepare(
        "SELECT s.name, s.rss_url, f.name
         FROM sources s
         LEFT JOIN folders f ON f.id = s.folder_id
         ORDER BY f.name NULLS LAST, s.name",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(FeedRow {
                name:        row.get(0)?,
                rss_url:     row.get(1)?,
                folder_name: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('"', "&quot;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
}

/// Exporta todas las suscripciones en formato OPML 1.0.
/// Los feeds agrupados en carpetas se exportan dentro de su outline padre;
/// los sin carpeta van en el nivel raíz.
async fn export_opml(
    State(state): State<AppState>,
    _user: AuthUser,
) -> AppResult<Response<Body>> {
    let feeds = load_feeds(&state)?;

    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="1.0">
  <head><title>Azkintun-RSS - suscripciones</title></head>
  <body>
"#,
    );

    // Agrupar por carpeta manteniendo el orden
    let mut current_folder: Option<String> = None;
    for feed in &feeds {
        let folder = feed.folder_name.as_deref();

        // Cerrar carpeta anterior si cambia
        if current_folder.as_deref() != folder {
            if current_folder.is_some() {
                out.push_str("  </outline>\n");
            }
            if let Some(f) = folder {
                out.push_str(&format!(
                    "  <outline text=\"{}\" title=\"{}\">\n",
                    xml_escape(f),
                    xml_escape(f)
                ));
                current_folder = Some(f.to_string());
            } else {
                current_folder = None;
            }
        }

        let indent = if current_folder.is_some() { "    " } else { "  " };
        out.push_str(&format!(
            "{}<outline type=\"rss\" text=\"{name}\" title=\"{name}\" xmlUrl=\"{url}\"/>\n",
            indent,
            name = xml_escape(&feed.name),
            url  = xml_escape(&feed.rss_url),
        ));
    }

    if current_folder.is_some() {
        out.push_str("  </outline>\n");
    }
    out.push_str("</body>\n</opml>\n");

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/x-opml; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"azkintun-subscriptions.opml\"",
        )
        .body(Body::from(out))
        .unwrap())
}

/// Exporta todas las suscripciones como CSV (Name, Feed URL, Folder).
/// Compatible con el importer de Azkintun-RSS e importable en hojas de cálculo.
async fn export_csv(
    State(state): State<AppState>,
    _user: AuthUser,
) -> AppResult<Response<Body>> {
    let feeds = load_feeds(&state)?;

    let mut out = String::from("Name,Feed URL,Folder\n");
    for feed in &feeds {
        let name   = csv_field(&feed.name);
        let url    = csv_field(&feed.rss_url);
        let folder = csv_field(feed.folder_name.as_deref().unwrap_or(""));
        out.push_str(&format!("{name},{url},{folder}\n"));
    }

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"azkintun-subscriptions.csv\"",
        )
        .body(Body::from(out))
        .unwrap())
}

/// Envuelve un campo CSV en comillas si contiene coma, comilla o newline.
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
