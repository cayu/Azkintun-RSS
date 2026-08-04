use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Error type unificado para toda la API. Cada variante se mapea a un
/// status code HTTP razonable; el mensaje siempre viaja como
/// `{ "error": "..." }` para que el cliente lo maneje de forma uniforme.
#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Unauthorized(String),
    Internal(anyhow::Error),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NotFound(m) => write!(f, "not found: {m}"),
            AppError::BadRequest(m) => write!(f, "bad request: {m}"),
            AppError::Conflict(m) => write!(f, "conflict: {m}"),
            AppError::Unauthorized(m) => write!(f, "unauthorized: {m}"),
            AppError::Internal(e) => write!(f, "internal error: {e}"),
        }
    }
}

// Nota: deliberadamente NO implementamos `std::error::Error` para
// `AppError`. Si lo hiciéramos, `AppError` satisfaría el blanket
// `impl<E: StdError + Send + Sync + 'static> From<E> for anyhow::Error`
// de la crate `anyhow`, y entonces nuestro propio `impl<E: Into<...>>
// From<E> for AppError` de más abajo entraría en conflicto real con el
// `impl<T> From<T> for T` reflexivo de la stdlib (E0119).

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            AppError::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

/// Cualquier error "anyhow" (rusqlite, reqwest, io, etc.) se convierte
/// automáticamente en un 500, salvo que se haya mapeado explícitamente
/// a otra variante antes con `.map_err(...)`.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        AppError::Internal(err.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;
