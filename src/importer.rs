use crate::models::ImportResult;
use crate::seeds::get_or_create_folder;
use crate::state::AppState;
use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;

pub struct ParsedFeed {
    name: String,
    url: String,
    folder: String,
}

/// Inoreader (y la mayoría de exports de "lectores" tipo Feedly/NewsBlur)
/// no tienen un CSV verdaderamente estandarizado, así que este parser
/// detecta columnas por nombre de forma flexible en vez de asumir un
/// orden fijo. Reconoce variantes en inglés y español para:
///   - la URL del feed:    url, feed url, rss url, xmlurl, link, feedurl
///   - el nombre del feed: title, name, feed title, nombre
///   - la carpeta:         folder, category, categoria, carpeta, tags
fn find_column<'a>(headers: &'a [String], candidates: &[&str]) -> Option<usize> {
    headers.iter().position(|h| {
        // Quitar un posible BOM UTF-8 (\u{feff}) que algunos programas
        // (Excel, exports de Windows) anteponen a la primera celda del
        // header, y que rompería la comparación de la primera columna.
        let h = h.trim_start_matches('\u{feff}').trim().to_lowercase();
        candidates.iter().any(|c| h == *c)
    })
}

pub fn parse_csv(data: &[u8]) -> Result<(Vec<ParsedFeed>, Vec<String>)> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(data);

    let headers: Vec<String> = reader
        .headers()?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let url_idx = find_column(
        &headers,
        &["url", "feed url", "rss url", "xmlurl", "feedurl", "link"],
    );
    let name_idx = find_column(
        &headers,
        &["title", "name", "feed title", "feedtitle", "nombre"],
    );
    let folder_idx = find_column(
        &headers,
        &["folder", "folders", "category", "categoria", "carpeta", "tags"],
    );

    let mut errors = Vec::new();
    let Some(url_idx) = url_idx else {
        errors.push(
            "No se encontró una columna de URL (esperaba algo como 'url', 'feed url', 'xmlUrl')."
                .to_string(),
        );
        return Ok((vec![], errors));
    };

    let mut feeds = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("Fila {}: {e}", i + 2));
                continue;
            }
        };

        let Some(url) = record.get(url_idx).map(|s| s.trim().to_string()) else {
            continue;
        };
        if url.is_empty() {
            continue;
        }

        let name = name_idx
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| host_from_url(&url));

        let folder = folder_idx
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Imported".to_string());

        feeds.push(ParsedFeed { name, url, folder });
    }

    Ok((feeds, errors))
}

fn host_from_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

fn attr_value(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.as_ref().eq_ignore_ascii_case(name.as_bytes()) {
            String::from_utf8(a.value.to_vec()).ok()
        } else {
            None
        }
    })
}

/// Parsea un archivo OPML (el formato real de export/import de Inoreader,
/// Feedly y prácticamente todo lector RSS). Las carpetas son los
/// `<outline>` que agrupan otros `<outline>` sin `xmlUrl` propio; los
/// feeds son los `<outline xmlUrl="...">` (con o sin hijos).
pub fn parse_opml(data: &[u8]) -> Result<(Vec<ParsedFeed>, Vec<String>)> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);

    let mut feeds = Vec::new();
    let errors = Vec::new();
    // Cada frame representa un <outline> abierto; Some(nombre) si es carpeta.
    let mut folder_stack: Vec<Option<String>> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.name().as_ref().eq_ignore_ascii_case(b"outline") => {
                let xml_url = attr_value(&e, "xmlUrl");
                let title = attr_value(&e, "title").or_else(|| attr_value(&e, "text"));

                if let Some(url) = xml_url {
                    let folder = folder_stack
                        .iter()
                        .rev()
                        .find_map(|f| f.clone())
                        .unwrap_or_else(|| "Imported".to_string());
                    let name = title.unwrap_or_else(|| host_from_url(&url));
                    feeds.push(ParsedFeed { name, url, folder });
                    folder_stack.push(None);
                } else {
                    folder_stack.push(Some(title.unwrap_or_else(|| "Imported".to_string())));
                }
            }
            Ok(Event::Empty(e)) if e.name().as_ref().eq_ignore_ascii_case(b"outline") => {
                if let Some(url) = attr_value(&e, "xmlUrl") {
                    let title = attr_value(&e, "title").or_else(|| attr_value(&e, "text"));
                    let folder = folder_stack
                        .iter()
                        .rev()
                        .find_map(|f| f.clone())
                        .unwrap_or_else(|| "Imported".to_string());
                    let name = title.unwrap_or_else(|| host_from_url(&url));
                    feeds.push(ParsedFeed { name, url, folder });
                }
            }
            Ok(Event::End(e)) if e.name().as_ref().eq_ignore_ascii_case(b"outline") => {
                folder_stack.pop();
            }
            Ok(_) => {}
            Err(e) => {
                return Err(anyhow::anyhow!("error parseando OPML: {e}"));
            }
        }
        buf.clear();
    }

    Ok((feeds, errors))
}

/// Inserta los feeds parseados en la base, creando las carpetas que
/// hagan falta. Es idempotente por `rss_url UNIQUE`: los que ya existen
/// se cuentan como "skipped", no se pisan.
pub fn insert_parsed_feeds(
    state: &AppState,
    feeds: Vec<ParsedFeed>,
    mut errors: Vec<String>,
) -> Result<ImportResult> {
    let mut conn = state.db.get()?;
    let tx = conn.transaction()?;

    let mut folder_cache: HashMap<String, i64> = HashMap::new();
    let mut folders_created = 0usize;
    let mut sources_created = 0usize;
    let mut sources_skipped = 0usize;

    for feed in feeds {
        let folder_id = if let Some(id) = folder_cache.get(&feed.folder) {
            *id
        } else {
            let existed: bool = tx
                .query_row(
                    "SELECT 1 FROM folders WHERE name = ?1",
                    [&feed.folder],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            let id = get_or_create_folder(&tx, &feed.folder)?;
            if !existed {
                folders_created += 1;
            }
            folder_cache.insert(feed.folder.clone(), id);
            id
        };

        let changes = tx
            .execute(
                "INSERT OR IGNORE INTO sources (name, rss_url, folder_id, custom) VALUES (?1, ?2, ?3, 1)",
                rusqlite::params![feed.name, feed.url, folder_id],
            )
            .unwrap_or(0);

        if changes > 0 {
            sources_created += 1;
        } else {
            sources_skipped += 1;
        }
    }

    tx.commit()?;

    if sources_created == 0 && sources_skipped == 0 && errors.is_empty() {
        errors.push("No se encontraron feeds válidos en el archivo.".to_string());
    }

    Ok(ImportResult {
        folders_created,
        sources_created,
        sources_skipped,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_detecta_columnas_por_nombre_sin_importar_orden() {
        // La URL está en la 3ra columna, el nombre en la 2da.
        let csv = b"Category,Feed Title,Feed URL\nTech,Ars,https://arstechnica.com/feed\n";
        let (feeds, errors) = parse_csv(csv).unwrap();
        assert!(errors.is_empty(), "no debería haber errores: {errors:?}");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].name, "Ars");
        assert_eq!(feeds[0].url, "https://arstechnica.com/feed");
        assert_eq!(feeds[0].folder, "Tech");
    }

    #[test]
    fn csv_maneja_bom_utf8_en_el_primer_header() {
        // Excel y varios exports de Windows anteponen un BOM al archivo,
        // que quedaría pegado al primer header ("\u{feff}Title").
        let csv = "\u{feff}Title,Feed url,Folder\nGraham,https://grahamcluley.com/feed/,Opinion\n"
            .as_bytes();
        let (feeds, errors) = parse_csv(csv).unwrap();
        assert!(errors.is_empty(), "BOM no debería romper la detección: {errors:?}");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].name, "Graham");
        assert_eq!(feeds[0].folder, "Opinion");
    }

    #[test]
    fn csv_sin_columna_url_reporta_error() {
        let csv = b"Title,Folder\nSomething,Tech\n";
        let (feeds, errors) = parse_csv(csv).unwrap();
        assert!(feeds.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn csv_usa_host_como_nombre_si_falta_title() {
        let csv = b"url\nhttps://example.com/path/feed.xml\n";
        let (feeds, _) = parse_csv(csv).unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].name, "example.com");
        assert_eq!(feeds[0].folder, "Imported");
    }

    #[test]
    fn opml_respeta_carpetas_anidadas_y_feeds_sueltos() {
        let opml = br#"<?xml version="1.0"?>
        <opml version="1.0"><body>
          <outline text="Tech" title="Tech">
            <outline text="Ars" title="Ars" type="rss" xmlUrl="https://arstechnica.com/feed"/>
          </outline>
          <outline text="Loose" title="Loose" type="rss" xmlUrl="https://example.org/rss.xml"/>
        </body></opml>"#;
        let (feeds, errors) = parse_opml(opml).unwrap();
        assert!(errors.is_empty());
        assert_eq!(feeds.len(), 2);

        let ars = feeds.iter().find(|f| f.name == "Ars").unwrap();
        assert_eq!(ars.folder, "Tech");

        let loose = feeds.iter().find(|f| f.name == "Loose").unwrap();
        assert_eq!(loose.folder, "Imported"); // sin carpeta padre -> Imported
    }

    #[test]
    fn opml_maneja_outline_feed_con_hijos() {
        // Algunos exports ponen el xmlUrl en un outline que además agrupa
        // otros (no debería tratarse como carpeta).
        let opml = br#"<opml><body>
          <outline title="Blog" type="rss" xmlUrl="https://blog.example.com/feed">
            <outline title="Nested" type="rss" xmlUrl="https://blog.example.com/nested"/>
          </outline>
        </body></opml>"#;
        let (feeds, _) = parse_opml(opml).unwrap();
        let names: Vec<_> = feeds.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"Blog"));
        assert!(names.contains(&"Nested"));
    }
}
