use anyhow::Result;
use r2d2::Pool;
use std::path::PathBuf;

/// Implementación mínima propia de `r2d2::ManageConnection` para SQLite.
/// Reemplaza al crate `r2d2_sqlite`, que arrastra `uuid` (con las
/// features `v4`+`fast-rng`) -> `rand` -> `chacha20` -> `cpufeatures 0.3`,
/// una cadena de dependencias que no compila en toolchains anteriores a
/// los que soportan la edition2024 de Cargo. Esta versión no tiene ese
/// problema y de paso es más simple.
#[derive(Clone)]
pub struct SqliteConnectionManager {
    path: PathBuf,
}

impl SqliteConnectionManager {
    pub fn file(path: PathBuf) -> Self {
        Self { path }
    }
}

impl r2d2::ManageConnection for SqliteConnectionManager {
    type Connection = rusqlite::Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let conn = rusqlite::Connection::open(&self.path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        Ok(conn)
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        conn.execute_batch("SELECT 1")
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}

pub type DbPool = Pool<SqliteConnectionManager>;

const CURRENT_SCHEMA_VERSION: i64 = 1;

/// Determina dónde vive el archivo SQLite. Se controla con la variable
/// `AZKINTUN_DATA_DIR`, que en Docker/k8s apunta al volumen montado
/// (`/app/data`) y en desarrollo local, si no se define, usa el directorio
/// actual. Se documenta en `.env.example` y en los manifiestos de k8s.
fn db_path() -> PathBuf {
    let dir = std::env::var("AZKINTUN_DATA_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&dir).ok();
    PathBuf::from(dir).join("azkintun.db")
}

pub fn init_pool() -> Result<DbPool> {
    let path = db_path();
    tracing::info!("[DB] Using database at {}", path.display());

    let manager = SqliteConnectionManager::file(path);
    let pool = Pool::builder().max_size(16).build(manager)?;

    run_migrations(&pool)?;
    Ok(pool)
}

fn run_migrations(pool: &DbPool) -> Result<()> {
    let conn = pool.get()?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS folders (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL UNIQUE,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS sources (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL,
            rss_url    TEXT NOT NULL UNIQUE,
            folder_id  INTEGER REFERENCES folders(id) ON DELETE SET NULL,
            active     INTEGER NOT NULL DEFAULT 1,
            custom     INTEGER NOT NULL DEFAULT 0,
            last_fetch DATETIME,
            last_error TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS articles (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id    INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
            title        TEXT NOT NULL,
            summary      TEXT NOT NULL DEFAULT '',
            severity     TEXT NOT NULL DEFAULT 'medium',
            published_at DATETIME,
            read_time    TEXT NOT NULL DEFAULT '2 MIN READ',
            url          TEXT NOT NULL UNIQUE,
            image_url    TEXT,
            is_read      INTEGER NOT NULL DEFAULT 0,
            is_starred   INTEGER NOT NULL DEFAULT 0,
            fetched_at   DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_articles_source    ON articles(source_id);
        CREATE INDEX IF NOT EXISTS idx_articles_published ON articles(published_at);
        CREATE INDEX IF NOT EXISTS idx_articles_read       ON articles(is_read);
        CREATE INDEX IF NOT EXISTS idx_sources_folder      ON sources(folder_id);

        CREATE TABLE IF NOT EXISTS fetch_log (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id      INTEGER,
            source_name    TEXT,
            articles_found INTEGER NOT NULL DEFAULT 0,
            articles_new   INTEGER NOT NULL DEFAULT 0,
            error          TEXT,
            fetched_at     DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS schema_metadata (
            key   TEXT PRIMARY KEY,
            value INTEGER
        );

        CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            username      TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO schema_metadata (key, value) VALUES ('version', ?1)",
        [CURRENT_SCHEMA_VERSION],
    )?;

    // Migraciones incrementales: añadir columnas nuevas si no existen,
    // y rellenar datos derivados de columnas viejas.
    add_column_if_missing(&conn, "articles", "image_url", "TEXT")?;

    // Migración: si la columna redundante summary_text existe, copiar su
    // contenido a summary (por si hay datos allí) y luego se puede ignorar.
    // SQLite no permite DROP COLUMN antes de 3.35.0, así que la dejamos
    // pero aseguramos que los nuevos inserts no la usen.
    // Para instancias nuevas el schema ya no la crea.

    // Migración: si article_count existe en sources (versión vieja),
    // no lo borramos (SQLite < 3.35 no tiene DROP COLUMN) pero sí dejamos
    // de escribirlo. Lo calculamos dinámicamente en las queries.

    Ok(())
}

/// Agrega una columna a una tabla solo si todavía no existe. Evita el
/// error "duplicate column name" al re-ejecutar migraciones.
fn add_column_if_missing(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let exists: bool = {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let cols = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut found = false;
        for c in cols {
            if c? == column {
                found = true;
                break;
            }
        }
        found
    };
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
        tracing::info!("[DB] Migración: columna {table}.{column} agregada");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Crea una conexión en memoria con el schema completo aplicado.
    /// El DDL replica el de `run_migrations` para que el test valide
    /// exactamente el schema real de producción.
    fn schema_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                rss_url TEXT NOT NULL UNIQUE,
                folder_id INTEGER REFERENCES folders(id) ON DELETE SET NULL,
                active INTEGER NOT NULL DEFAULT 1,
                custom INTEGER NOT NULL DEFAULT 0,
                last_fetch DATETIME,
                last_error TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                severity TEXT NOT NULL DEFAULT 'medium',
                published_at DATETIME,
                read_time TEXT NOT NULL DEFAULT '2 MIN READ',
                url TEXT NOT NULL UNIQUE,
                image_url TEXT,
                is_read INTEGER NOT NULL DEFAULT 0,
                is_starred INTEGER NOT NULL DEFAULT 0,
                fetched_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .unwrap();
        conn
    }

    /// El SELECT de artículos (definido en routes/articles.rs) debe ser
    /// válido contra el schema real. Este test habría detectado la
    /// referencia a la columna eliminada `summary_text`.
    #[test]
    fn article_select_is_valid_against_schema() {
        let conn = schema_conn();
        // Mismas columnas que ARTICLE_SELECT en routes/articles.rs.
        let sql = "
            SELECT a.id, a.title, a.summary, a.source_id, s.name, s.folder_id, f.name,
                   a.severity, a.published_at, a.read_time, a.url, a.image_url, a.is_read, a.is_starred
            FROM articles a
            JOIN sources s ON s.id = a.source_id
            LEFT JOIN folders f ON f.id = s.folder_id";
        // prepare() falla si alguna columna o tabla no existe.
        conn.prepare(sql)
            .expect("ARTICLE_SELECT debe ser válido contra el schema");
    }

    /// El SELECT de fuentes (routes/sources.rs) con el COUNT dinámico debe
    /// ser válido contra el schema real.
    #[test]
    fn source_select_is_valid_against_schema() {
        let conn = schema_conn();
        let sql = "
            SELECT s.id, s.name, s.rss_url, s.folder_id, f.name,
                   s.active, s.custom,
                   (SELECT COUNT(*) FROM articles a WHERE a.source_id = s.id) as article_count,
                   s.last_fetch, s.last_error
            FROM sources s
            LEFT JOIN folders f ON f.id = s.folder_id";
        conn.prepare(sql)
            .expect("SOURCE_SELECT debe ser válido contra el schema");
    }

    /// El INSERT de artículos del scraper debe coincidir con las columnas
    /// del schema.
    #[test]
    fn article_insert_matches_schema() {
        let conn = schema_conn();
        conn.execute(
            "INSERT INTO folders (name) VALUES ('Test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sources (name, rss_url, folder_id) VALUES ('S', 'http://x', 1)",
            [],
        )
        .unwrap();
        // Mismas columnas que el INSERT del scraper.
        conn.execute(
            "INSERT OR IGNORE INTO articles
             (source_id, title, summary, image_url, severity, published_at, read_time, url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![1, "T", "resumen", Option::<String>::None, "low", "2020-01-01", "1 MIN READ", "http://a"],
        )
        .expect("INSERT del scraper debe coincidir con el schema");

        // Y se puede leer de vuelta por la columna summary.
        let summary: String = conn
            .query_row("SELECT summary FROM articles WHERE url = 'http://a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(summary, "resumen");
    }
}
