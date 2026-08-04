use crate::auth::{create_token, hash_password, verify_password, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::{ChangePasswordRequest, LoginRequest, LoginResponse, UserInfo};
use crate::state::AppState;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

/// Rutas públicas de auth (no requieren token): login.
pub fn public_router() -> Router<AppState> {
    Router::new().route("/api/auth/login", post(login))
}

/// Rutas de auth que requieren estar autenticado.
pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/me", get(me))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/change-password", post(change_password))
}

/// Construye el valor de la cookie `access_token` (httpOnly, SameSite=Strict).
/// `max_age` negativo/cero se usa para expirarla (logout).
fn build_cookie(token: &str, max_age_secs: i64, secure: bool) -> HeaderValue {
    let mut cookie = format!(
        "access_token={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age_secs}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    // El token es base64url + puntos → siempre ASCII válido para un header.
    HeaderValue::from_str(&cookie).expect("cookie header is valid ASCII")
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> AppResult<impl IntoResponse> {
    let username = body.username.trim().to_string();
    if username.is_empty() || body.password.is_empty() {
        return Err(AppError::BadRequest("username and password are required".into()));
    }

    // Buscar el usuario. Nota: devolvemos el mismo error tanto si el
    // usuario no existe como si la contraseña es incorrecta, para no
    // filtrar qué usuarios existen.
    let row: Option<(i64, String)> = {
        let conn = state.db.get()?;
        conn.query_row(
            "SELECT id, password_hash FROM users WHERE username = ?1",
            [&username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    };

    let (user_id, password_hash) = match row {
        Some(v) => v,
        None => {
            // Verificación "dummy" para tiempo de respuesta constante
            // (evita enumerar usuarios por timing).
            let _ = verify_password(&body.password, DUMMY_HASH);
            // Delay adicional en fallos: ralentiza ataques de diccionario
            // sin estado compartido ni complejidad extra.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            return Err(AppError::Unauthorized("Invalid username or password".into()));
        }
    };

    if !verify_password(&body.password, &password_hash) {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        return Err(AppError::Unauthorized("Invalid username or password".into()));
    }

    let token = create_token(
        user_id,
        &username,
        &state.auth.jwt_secret,
        state.auth.token_ttl_secs,
    )?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        build_cookie(&token, state.auth.token_ttl_secs as i64, state.auth.cookie_secure),
    );

    let resp = LoginResponse {
        token,
        expires_in: state.auth.token_ttl_secs,
        user: UserInfo {
            id: user_id,
            username,
        },
    };
    Ok((headers, Json(resp)))
}

async fn logout(State(state): State<AppState>) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    // Expira la cookie inmediatamente.
    headers.insert(
        header::SET_COOKIE,
        build_cookie("", 0, state.auth.cookie_secure),
    );
    (headers, Json(serde_json::json!({ "success": true })))
}

async fn me(user: AuthUser) -> Json<UserInfo> {
    Json(UserInfo {
        id: user.id,
        username: user.username,
    })
}

async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ChangePasswordRequest>,
) -> AppResult<impl IntoResponse> {
    if body.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "New password must be at least 8 characters".into(),
        ));
    }

    let current_hash: String = {
        let conn = state.db.get()?;
        conn.query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            [user.id],
            |r| r.get(0),
        )
        .map_err(|_| AppError::NotFound("User not found".into()))?
    };

    if !verify_password(&body.current_password, &current_hash) {
        return Err(AppError::Unauthorized("Current password is incorrect".into()));
    }

    let new_hash = hash_password(&body.new_password)?;
    {
        let conn = state.db.get()?;
        conn.execute(
            "UPDATE users SET password_hash = ?1 WHERE id = ?2",
            rusqlite::params![new_hash, user.id],
        )?;
    }

    Ok((StatusCode::OK, Json(serde_json::json!({ "success": true }))))
}

/// Hash Argon2id de una contraseña placeholder, usado solo para gastar
/// tiempo de CPU comparable cuando el usuario no existe (mitiga
/// enumeración de usuarios por timing). Es un hash válido cualquiera.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$b2xkZHVtbXloYXNodmFsdWVvZmNvcnJlY3RsZW5ndGg";
