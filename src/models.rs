use serde::{Deserialize, Serialize};

/// Una carpeta/categoría (estilo Inoreader) que agrupa fuentes RSS.
#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: i64,
    pub name: String,
    #[serde(rename = "sourceCount")]
    pub source_count: i64,
    #[serde(rename = "unreadCount")]
    pub unread_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFolderRequest {
    pub name: String,
}

/// Una fuente RSS suscrita, opcionalmente asignada a una carpeta.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub id: i64,
    pub name: String,
    #[serde(rename = "rssUrl")]
    pub rss_url: String,
    #[serde(rename = "folderId")]
    pub folder_id: Option<i64>,
    #[serde(rename = "folderName")]
    pub folder_name: Option<String>,
    pub active: bool,
    pub custom: bool,
    #[serde(rename = "articleCount")]
    pub article_count: i64,
    #[serde(rename = "lastFetch")]
    pub last_fetch: Option<String>,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSourceRequest {
    pub name: String,
    #[serde(rename = "rssUrl")]
    pub rss_url: String,
    #[serde(rename = "folderId")]
    pub folder_id: Option<i64>,
    /// Nombre de carpeta alternativo: si se manda y no existe, se crea.
    #[serde(rename = "folderName")]
    pub folder_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateSourceRequest {
    pub name: Option<String>,
    pub active: Option<bool>,
    /// Ausente = no tocar. `0` = quitar de cualquier carpeta (queda
    /// "Uncategorized"). Cualquier otro valor = mover a esa carpeta.
    /// (Evita la ambigüedad de `Option<Option<i64>>` con serde, que no
    /// distingue un `null` explícito de un campo ausente.)
    #[serde(rename = "folderId")]
    pub folder_id: Option<i64>,
}

/// Un artículo scrapeado de una fuente RSS.
#[derive(Debug, Clone, Serialize)]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub summary: String,
    #[serde(rename = "sourceId")]
    pub source_id: i64,
    #[serde(rename = "sourceName")]
    pub source_name: String,
    #[serde(rename = "folderId")]
    pub folder_id: Option<i64>,
    #[serde(rename = "folderName")]
    pub folder_name: Option<String>,
    pub severity: String,
    #[serde(rename = "publishedAt")]
    pub published_at: Option<String>,
    #[serde(rename = "readTime")]
    pub read_time: String,
    pub url: String,
    #[serde(rename = "imageUrl")]
    pub image_url: Option<String>,
    #[serde(rename = "isRead")]
    pub is_read: bool,
    #[serde(rename = "isStarred")]
    pub is_starred: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct ArticleQuery {
    #[serde(rename = "folderId")]
    pub folder_id: Option<i64>,
    #[serde(rename = "sourceId")]
    pub source_id: Option<i64>,
    pub search: Option<String>,
    #[serde(rename = "unreadOnly")]
    pub unread_only: Option<bool>,
    #[serde(rename = "starredOnly")]
    pub starred_only: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateArticleRequest {
    #[serde(rename = "isRead")]
    pub is_read: Option<bool>,
    #[serde(rename = "isStarred")]
    pub is_starred: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    #[serde(rename = "foldersCreated")]
    pub folders_created: usize,
    #[serde(rename = "sourcesCreated")]
    pub sources_created: usize,
    #[serde(rename = "sourcesSkipped")]
    pub sources_skipped: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ScrapeStatus {
    pub scraping: bool,
    #[serde(rename = "startedAt")]
    pub started_at: Option<String>,
    #[serde(rename = "lastFinishedAt")]
    pub last_finished_at: Option<String>,
    #[serde(rename = "lastTotalNew")]
    pub last_total_new: i64,
    #[serde(rename = "lastErrors")]
    pub last_errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    #[serde(rename = "totalArticles")]
    pub total_articles: i64,
    #[serde(rename = "totalSources")]
    pub total_sources: i64,
    #[serde(rename = "totalFolders")]
    pub total_folders: i64,
    #[serde(rename = "unreadArticles")]
    pub unread_articles: i64,
    #[serde(rename = "lastFetch")]
    pub last_fetch: Option<String>,
}

// ─────────────────────────── Auth ───────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// El JWT también viaja en una cookie httpOnly; se devuelve en el body
    /// para clientes no-navegador (API/mobile).
    pub token: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: u64,
    pub user: UserInfo,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    #[serde(rename = "currentPassword")]
    pub current_password: String,
    #[serde(rename = "newPassword")]
    pub new_password: String,
}
