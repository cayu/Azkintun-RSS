use crate::error::AppError;
use crate::state::AppState;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Llena un buffer con bytes aleatorios criptográficamente seguros del OS.
fn os_random(buf: &mut [u8]) {
    getrandom::fill(buf).expect("OS RNG (getrandom) no disponible");
}

// ─────────────────────────── Password hashing ───────────────────────────

/// Hashea una contraseña con Argon2id (parámetros por defecto de la crate
/// `argon2` 0.5: m=19 MiB, t=2, p=1), con salt aleatorio del OS. El hash
/// resultante es un string PHC autocontenido (incluye algoritmo, params y
/// salt), listo para guardar en la DB.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let mut salt_bytes = [0u8; 16];
    os_random(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| anyhow::anyhow!("salt encode error: {e}"))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash error: {e}"))?
        .to_string();
    Ok(hash)
}

/// Verifica una contraseña contra un hash PHC. La verificación de Argon2
/// es intencionalmente lenta (mitiga fuerza bruta) y de tiempo constante
/// respecto al hash, así que no filtra información por timing.
pub fn verify_password(password: &str, phc_hash: &str) -> bool {
    match PasswordHash::new(phc_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

// ─────────────────────────── JWT (HS256) ───────────────────────────
//
// JWT armado a mano sobre HMAC-SHA256 (crates `hmac` + `sha2` de
// RustCrypto). Se evita `jsonwebtoken`/`ring` a propósito para no
// arrastrar dependencias con C/edition2024. El envelope JWT es trivial
// y la parte criptográfica (HMAC-SHA256) sí usa primitivas auditadas.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// user id
    pub sub: i64,
    pub username: String,
    /// issued-at (unix seconds)
    pub iat: u64,
    /// expiry (unix seconds)
    pub exp: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sign(signing_input: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(signing_input.as_bytes());
    B64.encode(mac.finalize().into_bytes())
}

/// Crea un JWT firmado para el usuario dado, válido por `ttl_secs`.
pub fn create_token(user_id: i64, username: &str, secret: &[u8], ttl_secs: u64) -> anyhow::Result<String> {
    let now = now_unix();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now,
        exp: now + ttl_secs,
    };
    // Header fijo: HS256. No aceptamos "alg" del cliente al verificar, así
    // que no hay riesgo de alg-confusion / "alg:none".
    let header = B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = B64.encode(serde_json::to_vec(&claims)?);
    let signing_input = format!("{header}.{payload}");
    let signature = sign(&signing_input, secret);
    Ok(format!("{signing_input}.{signature}"))
}

/// Verifica firma + expiración y devuelve los claims. Cualquier fallo
/// (formato, firma inválida, expirado) es un 401.
pub fn decode_token(token: &str, secret: &[u8]) -> Result<Claims, AppError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::Unauthorized("malformed token".into()));
    }
    let signing_input = format!("{}.{}", parts[0], parts[1]);

    // Verificación de firma en tiempo constante (Mac::verify_slice).
    let provided_sig = B64
        .decode(parts[2])
        .map_err(|_| AppError::Unauthorized("invalid token signature encoding".into()))?;
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&provided_sig)
        .map_err(|_| AppError::Unauthorized("invalid token signature".into()))?;

    let payload = B64
        .decode(parts[1])
        .map_err(|_| AppError::Unauthorized("invalid token payload encoding".into()))?;
    let claims: Claims = serde_json::from_slice(&payload)
        .map_err(|_| AppError::Unauthorized("invalid token claims".into()))?;

    if claims.exp < now_unix() {
        return Err(AppError::Unauthorized("token expired".into()));
    }
    Ok(claims)
}

// ─────────────────────────── Random helpers ───────────────────────────

/// Genera N bytes aleatorios del OS, en hex. Se usa para el JWT secret de
/// fallback y para contraseñas de admin autogeneradas.
pub fn random_hex(n_bytes: usize) -> String {
    let mut buf = vec![0u8; n_bytes];
    os_random(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

// ─────────────────────────── Extracción de token ───────────────────────────

/// Busca el token en `Authorization: Bearer <token>` o, si no está, en la
/// cookie httpOnly `access_token`. Soportar ambos permite:
///  - clientes de API / mobile → header Authorization
///  - navegador (SPA) → cookie httpOnly (inmune a robo por XSS)
fn extract_token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(rest) = s.strip_prefix("Bearer ").or_else(|| s.strip_prefix("bearer ")) {
                let t = rest.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    if let Some(cookie_header) = headers.get(axum::http::header::COOKIE) {
        if let Ok(s) = cookie_header.to_str() {
            for pair in s.split(';') {
                let pair = pair.trim();
                if let Some(val) = pair.strip_prefix("access_token=") {
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
}

// ─────────────────────────── Middleware + extractor ───────────────────────────

/// Usuario autenticado, disponible para los handlers vía `Extension`.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
}

/// Middleware que protege un subárbol de rutas: exige un JWT válido e
/// inyecta `AuthUser` en las extensiones del request. Si falta o es
/// inválido, corta con 401 sin llegar al handler.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = extract_token_from_headers(req.headers())
        .ok_or_else(|| AppError::Unauthorized("authentication required".into()))?;
    let claims = decode_token(&token, &state.auth.jwt_secret)?;
    req.extensions_mut().insert(AuthUser {
        id: claims.sub,
        username: claims.username,
    });
    Ok(next.run(req).await)
}

/// Extractor para handlers que necesitan saber quién es el usuario.
/// Depende de que `require_auth` haya corrido antes (que es el caso para
/// todas las rutas protegidas).
#[axum::async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(|| AppError::Unauthorized("authentication required".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_y_verify_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2id$"), "debe ser Argon2id: {hash}");
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("contraseña incorrecta", &hash));
    }

    #[test]
    fn dos_hashes_de_la_misma_pass_son_distintos() {
        // salt aleatorio => hashes distintos, ambos válidos
        let h1 = hash_password("misma").unwrap();
        let h2 = hash_password("misma").unwrap();
        assert_ne!(h1, h2);
        assert!(verify_password("misma", &h1));
        assert!(verify_password("misma", &h2));
    }

    #[test]
    fn jwt_roundtrip() {
        let secret = b"un-secreto-de-prueba-suficientemente-largo";
        let token = create_token(42, "alice", secret, 3600).unwrap();
        let claims = decode_token(&token, secret).unwrap();
        assert_eq!(claims.sub, 42);
        assert_eq!(claims.username, "alice");
    }

    #[test]
    fn jwt_firma_invalida_falla() {
        let token = create_token(1, "bob", b"secreto-uno-largo-para-test-1234", 3600).unwrap();
        let res = decode_token(&token, b"secreto-dos-distinto-largo-test-1");
        assert!(res.is_err(), "debe rechazar firma con otro secreto");
    }

    #[test]
    fn jwt_expirado_falla() {
        let secret = b"secreto-para-test-de-expiracion-1234";
        // ttl = 0 => exp == iat == ahora, ya vencido en el chequeo (<)
        // Forzamos exp en el pasado creando el token y esperando; en su
        // lugar construimos claims manualmente vía create_token con ttl 0
        // y validamos que 0 segundos de vida ya no sirva un instante después.
        let token = create_token(1, "x", secret, 0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(decode_token(&token, secret).is_err(), "token con ttl 0 debe expirar");
    }

    #[test]
    fn jwt_malformado_falla() {
        let secret = b"secreto-cualquiera-para-test-1234567";
        assert!(decode_token("no-es-un-jwt", secret).is_err());
        assert!(decode_token("a.b", secret).is_err());
        assert!(decode_token("a.b.c.d", secret).is_err());
    }

    #[test]
    fn random_hex_longitud_correcta() {
        assert_eq!(random_hex(16).len(), 32); // 16 bytes => 32 hex chars
        assert_ne!(random_hex(16), random_hex(16));
    }
}
