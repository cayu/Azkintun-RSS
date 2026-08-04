mod auth;
mod db;
mod error;
mod importer;
mod models;
mod routes;
mod scheduler;
mod scraper;
mod seeds;
mod seeds_data;
mod state;

use anyhow::Result;
use state::AppState;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let pool = db::init_pool()?;
    seeds::seed_sources(&pool)?;
    bootstrap_admin(&pool)?;

    let state = AppState::new(pool);

    tracing::info!(
        "[SERVER] Auth: JWT (Argon2id) | cookie Secure: {} | token TTL: {}h",
        state.auth.cookie_secure,
        state.auth.token_ttl_secs / 3600
    );

    scheduler::spawn_background_scraper(state.clone());

    let app = routes::all_routes(state.clone())
        .layer(build_cors())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("[SERVER] Azkintun-RSS escuchando en http://{addr}");
    tracing::info!("[SERVER] API: http://{addr}/api");

    axum::serve(listener, app).await?;
    Ok(())
}

/// CORS restrictivo. En la arquitectura recomendada (nginx sirve el
/// frontend y hace proxy de `/api` → mismo origen) NO hace falta CORS.
/// Para escenarios donde el frontend corre en otro origen (dev), se
/// habilita solo el origen exacto de `CORS_ALLOWED_ORIGIN`, con
/// credentials, que es lo único compatible con cookies de sesión
/// (un `*` con credenciales está prohibido por la spec y sería inseguro).
fn build_cors() -> tower_http::cors::CorsLayer {
    use tower_http::cors::CorsLayer;
    match std::env::var("CORS_ALLOWED_ORIGIN") {
        Ok(origin) if !origin.trim().is_empty() => {
            match origin.parse::<axum::http::HeaderValue>() {
                Ok(hv) => {
                    tracing::info!("[CORS] Permitiendo origen: {origin}");
                    CorsLayer::new()
                        .allow_origin(hv)
                        .allow_credentials(true)
                        .allow_methods(tower_http::cors::Any)
                        .allow_headers([
                            axum::http::header::CONTENT_TYPE,
                            axum::http::header::AUTHORIZATION,
                        ])
                }
                Err(_) => {
                    tracing::warn!("[CORS] CORS_ALLOWED_ORIGIN inválido, CORS deshabilitado");
                    CorsLayer::new()
                }
            }
        }
        _ => {
            // Sin CORS: solo mismo origen (el caso del proxy nginx).
            CorsLayer::new()
        }
    }
}

/// Crea el usuario admin inicial si no existe ninguno. Toma
/// `ADMIN_USERNAME` (default "admin") y `ADMIN_PASSWORD`; si esta última
/// no se define, genera una aleatoria y la imprime UNA vez en el log.
fn bootstrap_admin(pool: &db::DbPool) -> Result<()> {
    let conn = pool.get()?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let username = std::env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
    let (password, generated) = match std::env::var("ADMIN_PASSWORD") {
        Ok(p) if !p.is_empty() => (p, false),
        _ => (auth::random_hex(12), true),
    };

    let hash = auth::hash_password(&password)?;
    conn.execute(
        "INSERT INTO users (username, password_hash) VALUES (?1, ?2)",
        rusqlite::params![username, hash],
    )?;

    if generated {
        tracing::warn!(
            "[AUTH] Usuario admin creado: '{username}' con contraseña GENERADA: {password}"
        );
        tracing::warn!("[AUTH] Guardala ya y cambiala con POST /api/auth/change-password.");
    } else {
        tracing::info!("[AUTH] Usuario admin creado: '{username}'");
    }
    Ok(())
}
