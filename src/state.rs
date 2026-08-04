use crate::auth::random_hex;
use crate::db::DbPool;
use crate::models::ScrapeStatus;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuración de autenticación derivada del entorno.
#[derive(Clone)]
pub struct AuthConfig {
    /// Secreto para firmar los JWT (HMAC-SHA256).
    pub jwt_secret: Arc<Vec<u8>>,
    /// Vida útil de un token, en segundos.
    pub token_ttl_secs: u64,
    /// Si la cookie `access_token` lleva el flag `Secure` (obligatorio
    /// detrás de HTTPS; debe ser false para pruebas en http://localhost).
    pub cookie_secure: bool,
}

impl AuthConfig {
    fn from_env() -> Self {
        // JWT_SECRET: obligatorio en producción. Si falta o es corto,
        // generamos uno aleatorio y avisamos fuerte (los tokens no
        // sobrevivirán a un reinicio ni sirven con múltiples instancias).
        let jwt_secret = match std::env::var("JWT_SECRET") {
            Ok(s) if s.len() >= 32 => s.into_bytes(),
            Ok(_) => {
                tracing::warn!(
                    "[AUTH] JWT_SECRET demasiado corto (<32 caracteres). Generando uno temporal. \
                     Definí un JWT_SECRET fuerte y estable para producción."
                );
                random_hex(32).into_bytes()
            }
            Err(_) => {
                tracing::warn!(
                    "[AUTH] JWT_SECRET no definido. Generando uno temporal - los tokens NO \
                     sobrevivirán a un reinicio. Definí JWT_SECRET para producción."
                );
                random_hex(32).into_bytes()
            }
        };

        let token_ttl_secs = std::env::var("JWT_TTL_HOURS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|h| *h > 0)
            .unwrap_or(24)
            * 3600;

        // Secure por defecto (postura segura). Se apaga explícitamente con
        // COOKIE_SECURE=false para desarrollo local sobre http.
        let cookie_secure = std::env::var("COOKIE_SECURE")
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);

        Self {
            jwt_secret: Arc::new(jwt_secret),
            token_ttl_secs,
            cookie_secure,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub http: reqwest::Client,
    pub scrape_state: Arc<RwLock<ScrapeStatus>>,
    pub auth: AuthConfig,
}

impl AppState {
    pub fn new(db: DbPool) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Azkintun-RSS/1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("failed to build http client");

        Self {
            db,
            http,
            scrape_state: Arc::new(RwLock::new(ScrapeStatus {
                scraping: false,
                started_at: None,
                last_finished_at: None,
                last_total_new: 0,
                last_errors: vec![],
            })),
            auth: AuthConfig::from_env(),
        }
    }
}
