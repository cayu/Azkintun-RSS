use crate::state::AppState;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use regex::Regex;

pub struct SourceToScrape {
    pub id: i64,
    pub name: String,
    pub rss_url: String,
}

pub struct ScrapeOutcome {
    pub found: usize,
    pub new: usize,
    pub error: Option<String>,
}

static CRITICAL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)critical|0-day|zero-day|ransomware|breach|exploit|actively exploited|emergency").unwrap()
});
static HIGH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)high|vulnerability|flaw|attack|malware|phishing|cve").unwrap());
static MEDIUM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)medium|warning|alert|advisory|patch").unwrap());

/// Igual que `inferSeverity` en el original: heurística por palabras clave
/// en el título. No es ciencia exacta, es una primera pasada útil para
/// ordenar/filtrar visualmente el feed.
fn infer_severity(title: &str) -> &'static str {
    if CRITICAL_RE.is_match(title) {
        "critical"
    } else if HIGH_RE.is_match(title) {
        "high"
    } else if MEDIUM_RE.is_match(title) {
        "medium"
    } else {
        "low"
    }
}

/// Cuenta palabras en el resumen (sin tags HTML) y estima minutos de
/// lectura, igual que el original (mínimo 2 minutos).
fn read_time(summary: &str) -> String {
    let plain = strip_html(summary);
    let words = plain.split_whitespace().count().max(1);
    let minutes = ((words as f64) / 200.0).ceil().max(2.0) as i64;
    format!("{minutes} MIN READ")
}

fn strip_html(input: &str) -> String {
    static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());
    TAG_RE.replace_all(input, " ").to_string()
}

/// Convierte HTML a un resumen en texto plano: quita tags, decodifica las
/// entidades HTML más comunes, colapsa espacios y recorta a ~280 chars
/// (suficiente para el preview estilo Inoreader).
fn plain_summary(html: &str) -> String {
    let text = strip_html(html);
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&hellip;", "…");
    // colapsar espacios/whitespace repetidos
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed: String = collapsed.chars().take(280).collect();
    if collapsed.chars().count() > 280 {
        format!("{}…", trimmed.trim_end())
    } else {
        trimmed
    }
}

/// Extrae la mejor imagen disponible para un item, probando en orden:
///   1. Media RSS: `media:content` con type de imagen (o url que parece imagen)
///   2. Media RSS: `media:thumbnail`
///   3. Enclosures (`links` con rel="enclosure" y media_type de imagen)
///   4. Primer `<img src="…">` embebido en el contenido/resumen HTML
fn extract_image(item: &feed_rs::model::Entry, html_body: &str) -> Option<String> {
    let looks_like_image = |url: &str| {
        let u = url.split('?').next().unwrap_or(url).to_lowercase();
        u.ends_with(".jpg") || u.ends_with(".jpeg") || u.ends_with(".png")
            || u.ends_with(".webp") || u.ends_with(".gif")
    };

    for media in &item.media {
        for content in &media.content {
            if let Some(url) = &content.url {
                let is_image = content
                    .content_type
                    .as_ref()
                    .map(|t| t.ty() == "image")
                    .unwrap_or_else(|| looks_like_image(url.as_str()));
                if is_image {
                    return Some(url.to_string());
                }
            }
        }
        if let Some(thumb) = media.thumbnails.first() {
            return Some(thumb.image.uri.clone());
        }
    }

    for link in &item.links {
        if link.rel.as_deref() == Some("enclosure") {
            let is_image = link
                .media_type
                .as_deref()
                .map(|t| t.starts_with("image/"))
                .unwrap_or_else(|| looks_like_image(&link.href));
            if is_image {
                return Some(link.href.clone());
            }
        }
    }

    // Fallback: primer <img src> del HTML del contenido.
    static IMG_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?i)<img[^>]+src\s*=\s*["']([^"']+)["']"#).unwrap());
    if let Some(caps) = IMG_RE.captures(html_body) {
        let url = caps.get(1)?.as_str();
        if url.starts_with("http") {
            return Some(url.to_string());
        }
    }

    None
}

/// Descarga y parsea un feed RSS/Atom, e inserta artículos nuevos.
/// Usa `INSERT OR IGNORE` sobre `url UNIQUE` para deduplicar, igual que
/// el original (así se puede correr el mismo scrape muchas veces sin
/// generar duplicados).
pub async fn scrape_source(state: &AppState, source: &SourceToScrape) -> ScrapeOutcome {
    match scrape_source_inner(state, source).await {
        Ok(outcome) => outcome,
        Err(e) => {
            let error = format!("{e:#}");
            record_source_error(state, source, &error).await;
            ScrapeOutcome {
                found: 0,
                new: 0,
                error: Some(error),
            }
        }
    }
}

async fn scrape_source_inner(state: &AppState, source: &SourceToScrape) -> Result<ScrapeOutcome> {
    let bytes = state
        .http
        .get(&source.rss_url)
        .send()
        .await
        .with_context(|| format!("fetching {}", source.rss_url))?
        .error_for_status()
        .with_context(|| format!("http error from {}", source.rss_url))?
        .bytes()
        .await
        .context("reading response body")?;

    let feed = feed_rs::parser::parse(&bytes[..]).context("parsing feed")?;
    let items: Vec<_> = feed.entries.into_iter().take(10).collect();
    let found = items.len();

    let db = state.db.clone();
    let source_id = source.id;
    let source_name = source.name.clone();

    let new_count = tokio::task::spawn_blocking(move || -> Result<usize> {
        let mut conn = db.get()?;
        let tx = conn.transaction()?;
        let mut new_count = 0usize;

        for item in &items {
            let Some(title) = item.title.as_ref().map(|t| t.content.clone()) else {
                continue;
            };
            let Some(link) = item.links.first().map(|l| l.href.clone()) else {
                continue;
            };

            let summary_raw = item
                .summary
                .as_ref()
                .map(|s| s.content.clone())
                .or_else(|| item.content.as_ref().and_then(|c| c.body.clone()))
                .unwrap_or_default();
            // HTML crudo original (para buscar imágenes embebidas).
            let html_body = item
                .content
                .as_ref()
                .and_then(|c| c.body.clone())
                .unwrap_or_else(|| summary_raw.clone());

            let plain_text = plain_summary(&summary_raw);
            let image_url = extract_image(item, &html_body);

            let published: Option<DateTime<Utc>> = item.published.or(item.updated);
            let published_str = published.unwrap_or_else(Utc::now).to_rfc3339();

            let changes = tx.execute(
                "INSERT OR IGNORE INTO articles
                 (source_id, title, summary, image_url, severity, published_at, read_time, url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    source_id,
                    title,
                    plain_text,
                    image_url,
                    infer_severity(&title),
                    published_str,
                    read_time(&summary_raw),
                    link,
                ],
            )?;
            new_count += changes;
        }

        tx.execute(
            "UPDATE sources SET last_fetch = CURRENT_TIMESTAMP, last_error = NULL WHERE id = ?1",
            [source_id],
        )?;
        tx.execute(
            "INSERT INTO fetch_log (source_id, source_name, articles_found, articles_new) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![source_id, source_name, found as i64, new_count as i64],
        )?;

        tx.commit()?;
        Ok(new_count)
    })
    .await??;

    Ok(ScrapeOutcome {
        found,
        new: new_count,
        error: None,
    })
}

/// Registra un error de scraping: actualiza `sources.last_error` e
/// inserta una fila en `fetch_log`, todo en una sola operación de DB.
/// Usa `spawn_blocking` para no bloquear el runtime async con I/O de
/// SQLite (antes esto usaba `std::thread::spawn(...).join()`, que
/// bloqueaba sincrónicamente un hilo del executor - un anti-patrón).
async fn record_source_error(state: &AppState, source: &SourceToScrape, error: &str) {
    let db = state.db.clone();
    let source_id = source.id;
    let source_name = source.name.clone();
    let error = error.to_string();

    let result = tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
        let conn = db.get().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;
        conn.execute(
            "UPDATE sources SET last_error = ?1 WHERE id = ?2",
            rusqlite::params![error, source_id],
        )?;
        conn.execute(
            "INSERT INTO fetch_log (source_id, source_name, articles_found, articles_new, error) VALUES (?1, ?2, 0, 0, ?3)",
            rusqlite::params![source_id, source_name, error],
        )?;
        Ok(())
    })
    .await;

    if let Err(e) = result {
        tracing::error!("[SCRAPER] no se pudo registrar el error de '{}': {e}", source.name);
    }
}

/// Scrapea todas las fuentes activas con concurrencia limitada. Con
/// cientos/miles de fuentes, hacerlo secuencialmente sería lentísimo;
/// procesamos hasta `SCRAPE_CONCURRENCY` (default 16) en paralelo. Cada
/// fetch tiene su propio timeout, así que una fuente lenta no bloquea al
/// resto. Devuelve el total de artículos nuevos y la lista de errores.
pub async fn scrape_all(state: &AppState) -> Result<(usize, Vec<String>)> {
    use futures::stream::{self, StreamExt};

    let db = state.db.clone();
    let sources: Vec<SourceToScrape> = tokio::task::spawn_blocking(move || -> Result<_> {
        let conn = db.get()?;
        let mut stmt =
            conn.prepare("SELECT id, name, rss_url FROM sources WHERE active = 1 ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            Ok(SourceToScrape {
                id: row.get(0)?,
                name: row.get(1)?,
                rss_url: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
    .await??;

    let concurrency: usize = std::env::var("SCRAPE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(16);

    tracing::info!(
        "[SCRAPER] Iniciando scrape de {} fuentes activas (concurrencia {})",
        sources.len(),
        concurrency
    );

    let total = sources.len();
    let results = stream::iter(sources.into_iter())
        .map(|source| async move {
            let outcome = scrape_source(state, &source).await;
            (source.name, outcome)
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;

    let mut total_new = 0usize;
    let mut errors = Vec::new();
    for (name, outcome) in results {
        if let Some(err) = outcome.error {
            errors.push(format!("{name}: {err}"));
        } else {
            total_new += outcome.new;
        }
    }

    tracing::info!(
        "[SCRAPER] Listo. {} artículos nuevos, {} errores de {} fuentes.",
        total_new,
        errors.len(),
        total
    );
    Ok((total_new, errors))
}

/// Igual que `cleanOldArticles`: deja como máximo 100 artículos por fuente.
pub async fn clean_old_articles(state: &AppState) -> Result<()> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = db.get()?;
        conn.execute_batch(
            r#"
            DELETE FROM articles
            WHERE id NOT IN (
                SELECT id FROM (
                    SELECT id, ROW_NUMBER() OVER (PARTITION BY source_id ORDER BY published_at DESC) as rn
                    FROM articles
                ) WHERE rn <= 100
            );
            "#,
        )?;
        Ok(())
    })
    .await??;
    tracing::info!("[SCRAPER] Artículos viejos limpiados");
    Ok(())
}


/// Mantiene fetch_log acotado a las últimas 2000 entradas para evitar que
/// crezca indefinidamente con scrapes periódicos de ~1400 fuentes.
pub async fn clean_fetch_log(state: &AppState) -> Result<()> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = db.get()?;
        conn.execute(
            "DELETE FROM fetch_log WHERE id NOT IN (
                SELECT id FROM fetch_log ORDER BY id DESC LIMIT 2000
            )",
            [],
        )?;
        Ok(())
    })
    .await??;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severidad_critica_por_keywords() {
        assert_eq!(infer_severity("Critical Zero-Day Exploit in OpenSSL"), "critical");
        assert_eq!(infer_severity("New ransomware campaign hits hospitals"), "critical");
        assert_eq!(infer_severity("Actively exploited flaw patched"), "critical");
    }

    #[test]
    fn severidad_alta_media_baja() {
        assert_eq!(infer_severity("New CVE affects routers"), "high");
        assert_eq!(infer_severity("Security advisory published"), "medium");
        assert_eq!(infer_severity("Weekly community newsletter roundup"), "low");
    }

    #[test]
    fn read_time_minimo_dos_minutos() {
        assert_eq!(read_time("short text"), "2 MIN READ");
        assert_eq!(read_time(""), "2 MIN READ");
    }

    #[test]
    fn read_time_escala_con_longitud() {
        let long = "word ".repeat(600); // 600 palabras / 200 = 3
        assert_eq!(read_time(&long), "3 MIN READ");
    }

    #[test]
    fn strip_html_quita_tags() {
        let out = strip_html("<p>Hello <b>world</b></p>");
        assert!(out.contains("Hello"));
        assert!(out.contains("world"));
        assert!(!out.contains('<'));
    }

    #[test]
    fn plain_summary_limpia_html_y_entidades() {
        let s = plain_summary("<p>Hola &amp; chau <b>mundo</b></p>");
        assert_eq!(s, "Hola & chau mundo");
    }

    #[test]
    fn plain_summary_colapsa_espacios_y_recorta() {
        let largo = "palabra ".repeat(100); // 800 chars
        let s = plain_summary(&largo);
        assert!(s.chars().count() <= 281, "debe recortar a ~280: {}", s.chars().count());
        assert!(s.ends_with('…'), "debe terminar con elipsis si se recortó");
    }
}
