use crate::db::DbPool;
use anyhow::Result;
use std::collections::HashMap;

use crate::seeds_data::DEFAULT_SOURCES;

/// Busca una carpeta por nombre o la crea, devolviendo su id.
/// Se usa tanto en el seeding inicial como en el import de CSV/OPML.
pub fn get_or_create_folder(conn: &rusqlite::Connection, name: &str) -> Result<i64> {
    let name = name.trim();
    let name = if name.is_empty() { "Uncategorized" } else { name };

    conn.execute(
        "INSERT OR IGNORE INTO folders (name) VALUES (?1)",
        [name],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM folders WHERE name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(id)
}

/// Inserta las fuentes RSS por defecto creando sus carpetas.
/// También corrige la carpeta de las fuentes que ya existen en la DB
/// pero fueron asignadas a una carpeta distinta en versiones anteriores:
/// las fuentes del export personal del usuario tienen prioridad.
pub fn seed_sources(pool: &DbPool) -> Result<()> {
    let mut conn = pool.get()?;
    let tx = conn.transaction()?;

    let mut folder_cache: HashMap<&str, i64> = HashMap::new();
    let mut inserted = 0usize;
    let mut relocated = 0usize;

    // Primero asegurarse de que todas las carpetas del seed existen.
    for (_, _, folder_name) in DEFAULT_SOURCES {
        if !folder_cache.contains_key(folder_name) {
            let id = get_or_create_folder(&tx, folder_name)?;
            folder_cache.insert(folder_name, id);
        }
    }

    for (name, rss_url, folder_name) in DEFAULT_SOURCES {
        let folder_id = folder_cache[folder_name];

        // INSERT: solo si no existe (dedup por rss_url UNIQUE).
        let inserted_now = tx.execute(
            "INSERT OR IGNORE INTO sources (name, rss_url, folder_id, custom) VALUES (?1, ?2, ?3, 0)",
            rusqlite::params![name, rss_url, folder_id],
        )?;
        inserted += inserted_now;

        // RELOCATE: si la fuente ya existía pero está en otra carpeta,
        // moverla a la carpeta definida en el seed. Esto corrige instancias
        // existentes sin necesidad de borrar nada.
        if inserted_now == 0 {
            let moved = tx.execute(
                "UPDATE sources SET folder_id = ?1 WHERE rss_url = ?2 AND folder_id IS NOT ?1",
                rusqlite::params![folder_id, rss_url],
            )?;
            relocated += moved;
        }
    }

    tx.commit()?;

    if relocated > 0 {
        tracing::info!(
            "[SEEDS] {} fuentes reubicadas a su carpeta correcta",
            relocated
        );
    }
    tracing::info!(
        "[SEEDS] {} fuentes nuevas insertadas (de {} definidas)",
        inserted,
        DEFAULT_SOURCES.len()
    );
    Ok(())
}
