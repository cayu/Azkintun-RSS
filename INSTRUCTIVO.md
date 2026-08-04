# Azkintun-RSS — Instructivo de arquitectura y deploy

Este documento explica **cómo está construida** la app y **cómo se despliega** en un cluster k3s casero. Está pensado como material de estudio: un proyecto concreto, de tamaño real pero sin complejidad artificial, con el que aprender Rust web y fundamentos de SRE/DevOps.

---

## Índice

1. [¿Qué es y qué hace?](#1-qué-es-y-qué-hace)
2. [Stack elegido y por qué](#2-stack-elegido-y-por-qué)
3. [Arquitectura del código](#3-arquitectura-del-código)
4. [Módulos Rust: recorrido detallado](#4-módulos-rust-recorrido-detallado)
5. [Base de datos y migraciones](#5-base-de-datos-y-migraciones)
6. [Autenticación: Argon2 + JWT a mano](#6-autenticación-argon2--jwt-a-mano)
7. [El scraper: concurrencia real con Tokio](#7-el-scraper-concurrencia-real-con-tokio)
8. [Frontend: SPA sin framework](#8-frontend-spa-sin-framework)
9. [Los tests: qué prueban y por qué](#9-los-tests-qué-prueban-y-por-qué)
10. [Docker: dos contenedores bien separados](#10-docker-dos-contenedores-bien-separados)
11. [Deploy en k3s](#11-deploy-en-k3s)
12. [Observaciones y estado](#12-observaciones-y-estado)

---

## 1. ¿Qué es y qué hace?

Azkintun-RSS es un **agregador personal de feeds RSS** orientado a ciberseguridad. Scrapea periódicamente ~1400 fuentes (blogs, CVEs, podcasts, newsletters), las clasifica por severidad, y las presenta en una interfaz de cards navegable.

Funcionalidades:
- Organización en carpetas, favoritos, lectura/no lectura.
- Búsqueda full-text dentro de los artículos almacenados.
- Import y export de suscripciones en OPML y CSV (compatible con Inoreader, Feedly, Miniflux).
- Scrape concurrente de hasta 16 feeds en paralelo, configurable.
- Autenticación con sesión persistente vía cookie httpOnly.

Lo que NO tiene (deliberadamente): multi-usuario, notificaciones push, traducción automática, sincronización con terceros. Eso mantiene el código legible.

---

## 2. Stack elegido y por qué

| Capa | Tecnología | Por qué |
|---|---|---|
| Lenguaje | Rust 2021 | Performance, seguridad de memoria, un binario estático |
| Runtime async | Tokio | Estándar de facto para Rust async; el scraper necesita I/O concurrente |
| Framework web | Axum 0.7 | Construido sobre Tokio+Hyper; ergonómico, sin magia implícita |
| Base de datos | SQLite (rusqlite bundled) | Cero infra extra; suficiente para un usuario; el archivo es el backup |
| Pool de conexiones | r2d2 (propio) | `r2d2_sqlite` arrastraba una cadena de deps incompatible con Rust 1.75 |
| Feeds | feed-rs | Parser RSS/Atom/JSON Feed robusto |
| Auth | argon2 + hmac/sha2 | Sin dependencias C; sin crates de JWT que necesiten edition2024 |
| Frontend | HTML + JS vanilla | Sin bundler, sin framework; el navegador ya es suficiente para un SPA simple |
| Proxy | nginx | Sirve el SPA y hace reverse-proxy de `/api`; termina el TLS en Docker |
| Contenedores | Docker Compose (dev) + k3s (prod) | Mismas imágenes en ambos entornos |

### Por qué SQLite y no Postgres

Un lector RSS personal tiene un solo escritor (el scraper) y un solo lector concurrente (el usuario). SQLite en modo WAL maneja eso perfectamente, con latencias de lectura de microsegundos. Postgres agrega una instancia más que operar, respaldar y monitorear - complejidad sin beneficio en este caso.

### Por qué Axum y no Actix-web

Axum es más nuevo y más simple conceptualmente: los handlers son funciones async normales, el state sharing es explícito con `State<T>`, y el sistema de extractores (el `FromRequestParts` que implementa `AuthUser`) es elegante y tipado. Actix-web es más rápido en benchmarks pero tiene un modelo de actores más complejo.

---

## 3. Arquitectura del código

```
src/
├── main.rs          — punto de entrada: inicialización y wiring
├── state.rs         — AppState compartido entre todos los handlers
├── error.rs         — tipo de error unificado (AppError → HTTP response)
├── models.rs        — structs de dominio: Article, Source, Folder, etc.
├── db.rs            — pool SQLite, schema, migraciones
├── auth.rs          — hashing, JWT, middleware require_auth, extractor AuthUser
├── scraper.rs       — fetch + parse de feeds RSS, insert en DB
├── scheduler.rs     — loop periódico de scrape en background (Tokio)
├── seeds.rs         — siembra inicial de carpetas y fuentes al arrancar
├── seeds_data.rs    — los 1390 feeds hardcodeados (generado, no editar a mano)
├── importer.rs      — parse de OPML y CSV para el endpoint /api/import
└── routes/
    ├── mod.rs       — ensambla el router: público vs. protegido
    ├── health.rs    — GET /api/health (público) y GET /api/stats
    ├── auth.rs      — POST /api/auth/login|logout|me|change-password
    ├── articles.rs  — GET/PATCH /api/articles (+ filtros, mark-all-read)
    ├── folders.rs   — CRUD /api/folders
    ├── sources.rs   — CRUD /api/sources
    ├── import.rs    — POST /api/import/opml|csv
    ├── export.rs    — GET /api/export/opml|csv
    └── scrape.rs    — POST /api/scrape y GET /api/scrape/status
```

El flujo de arranque en `main.rs` es lineal e intencional:

```rust
let pool = db::init_pool()?;        // crea el schema si no existe
seeds::seed_sources(&pool)?;        // inserta los 1390 feeds (idempotente)
bootstrap_admin(&pool)?;            // crea el usuario admin si no hay ninguno
let state = AppState::new(pool);    // construye el estado compartido
scheduler::spawn_background_scraper(state.clone());  // arranca el loop
let app = routes::all_routes(state) // ensambla el router
    .layer(build_cors())
    .layer(TraceLayer::new_for_http())
    .with_state(state);
axum::serve(listener, app).await?;
```

Cada paso puede fallar con `?` porque `main` devuelve `Result<()>`. Si algo falla en la inicialización, el proceso termina con un mensaje de error claro. No hay inicialización lazy ni efectos globales escondidos.

---

## 4. Módulos Rust: recorrido detallado

### `state.rs` — Estado compartido

El patrón central de Axum es pasar un `State<T>` a cada handler. Aquí `T` es `AppState`:

```rust
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,                          // pool de conexiones SQLite
    pub http: reqwest::Client,               // cliente HTTP reutilizable (con timeout)
    pub scrape_state: Arc<RwLock<ScrapeStatus>>,  // estado del scrape en curso
    pub auth: AuthConfig,                    // secreto JWT, TTL, cookie_secure
}
```

El `Clone` es barato porque todo lo caro vive detrás de `Arc`. El `RwLock` permite que múltiples handlers lean el estado del scrape (`GET /api/scrape/status`) sin bloquearse, mientras el scraper lo escribe.

`AuthConfig` se construye desde variables de entorno en `from_env()`. Si `JWT_SECRET` no está definido o es corto, genera uno temporal y lo avisa en el log - la app arranca igual, pero los tokens no sobreviven a un reinicio.

### `error.rs` — Errores como respuestas HTTP

El patrón más común en APIs Axum es definir un tipo de error propio que implemente `IntoResponse`:

```rust
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Unauthorized(String),
    Internal(anyhow::Error),
}
```

La implementación de `IntoResponse` mapea cada variante a un status code y serializa el mensaje como `{ "error": "..." }`. Esto asegura que **todas** las respuestas de error tengan el mismo formato, independientemente de dónde ocurra el error.

El truco clave es el `impl From<E> for AppError`:

```rust
impl<E> From<E> for AppError
where E: Into<anyhow::Error> {
    fn from(err: E) -> Self {
        AppError::Internal(err.into())
    }
}
```

Esto hace que el operador `?` funcione en los handlers para cualquier error de rusqlite, reqwest, IO, etc. - todos se convierten automáticamente en un `500`.

> **Nota:** deliberadamente NO implementamos `std::error::Error` para `AppError`. Si lo hiciéramos, el blanket impl de anyhow y el impl reflexivo `From<T> for T` de la stdlib entrarían en conflicto (E0119). El comentario en el código explica el razonamiento.

El tipo alias `AppResult<T> = Result<T, AppError>` mantiene los handlers limpios.

### `routes/mod.rs` — Dos capas de router

```rust
let public = Router::new()
    .merge(health::public_router())
    .merge(auth::public_router());     // solo /api/health y /api/auth/login

let protected = Router::new()
    .merge(/* todos los demás */)
    .route_layer(middleware::from_fn_with_state(state, require_auth));
                                       // ← el middleware se aplica SOLO acá

public.merge(protected)
```

`route_layer` aplica el middleware **solo a las rutas del router donde se declara**, no a todas. Así el health endpoint queda accesible sin token para los probes de Kubernetes.

### `routes/articles.rs` — SQL dinámico seguro

El endpoint `GET /api/articles` acepta varios filtros opcionales: carpeta, fuente, búsqueda, solo-no-leídos, etc. Esto requiere construir SQL dinámicamente. El patrón usado:

```rust
let mut sql = format!("{ARTICLE_SELECT} WHERE 1=1");
let mut params: Vec<SqlValue> = Vec::new();

if let Some(folder_id) = q.folder_id {
    sql.push_str(" AND s.folder_id = ?");
    params.push(SqlValue::Integer(folder_id));
}
if let Some(ref search) = q.search {
    sql.push_str(" AND (a.title LIKE ? OR a.summary LIKE ?)");
    let pattern = format!("%{search}%");
    params.push(SqlValue::Text(pattern.clone()));
    params.push(SqlValue::Text(pattern));
}
// ... más filtros
```

Los parámetros nunca se interpolan en el string SQL - siempre se pasan como valores separados a SQLite. Esto previene inyección SQL sin necesidad de un ORM.

### `models.rs` — Tipos de dominio con Serde

Los structs del dominio derivan `Serialize`/`Deserialize` y usan `#[serde(rename_all = "camelCase")]` para que el JSON que produce la API use camelCase (convención JS) mientras el código Rust usa snake_case:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub source_name: String,    // → "sourceName" en JSON
    pub is_read: bool,          // → "isRead" en JSON
    // ...
}
```

---

## 5. Base de datos y migraciones

### Pool de conexiones propio

`r2d2_sqlite` arrastraba una cadena de dependencias transitivas incompatible con Rust 1.75. En lugar de cambiar el toolchain, se implementó un `ManageConnection` mínimo directamente sobre `rusqlite`:

```rust
impl r2d2::ManageConnection for SqliteConnectionManager {
    type Connection = rusqlite::Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let conn = rusqlite::Connection::open(&self.path)?;
        // WAL mode: permite múltiples lectores concurrentes con un escritor
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        Ok(conn)
    }
    // ...
}
```

El pool tiene `max_size = 16`. Cada request saca una conexión del pool, la usa, y la devuelve. Con SQLite en WAL, las lecturas concurrentes no se bloquean entre sí.

### Schema y migraciones idempotentes

El schema se crea con `CREATE TABLE IF NOT EXISTS` - se puede re-ejecutar en cada arranque sin error. Para agregar columnas en versiones existentes se usa `add_column_if_missing`, que consulta `PRAGMA table_info` antes de hacer `ALTER TABLE`:

```rust
fn add_column_if_missing(conn, table, column, definition) {
    let exists = /* PRAGMA table_info(table) */;
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?;
    }
}
```

Este patrón funciona sin necesidad de una tabla de versiones de esquema compleja. Para el caso de uso actual (una instancia, un operador) es suficiente y muy transparente.

---

## 6. Autenticación: Argon2 + JWT a mano

La autenticación intencionalmente **no usa crates de alto nivel** como `jsonwebtoken` o `ring`. Motivo: esas crates tienen dependencias de C o requieren features de Cargo incompatibles con el toolchain usado. La implementación manual solo necesita `hmac`, `sha2`, y `base64` - todo Rust puro.

### Hashing de contraseñas

```rust
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let mut salt_bytes = [0u8; 16];
    getrandom::fill(&mut salt_bytes)?;      // aleatorio del OS, no rand
    let salt = SaltString::encode_b64(&salt_bytes)?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string();   // formato PHC: "$argon2id$v=19$m=19456,t=2,p=1$..."
    Ok(hash)
}
```

Argon2id es el algoritmo recomendado hoy para contraseñas. El formato PHC incluye el algoritmo, parámetros y salt en el mismo string - se puede verificar sin guardar el salt por separado.

### JWT a mano

El formato JWT tiene tres partes separadas por `.`:
```
BASE64URL(header).BASE64URL(payload).BASE64URL(signature)
```

La implementación construye el token a partir del id de usuario y sus datos:
```rust
pub fn create_token(user_id: i64, username: &str, secret: &[u8], ttl_secs: u64)
    -> anyhow::Result<String>
{
    let claims = Claims { sub: user_id, username: username.into(),
                          iat: now, exp: now + ttl_secs };
    // Header fijo HS256. Al verificar NO leemos el "alg" del cliente, así
    // que no hay riesgo de alg-confusion / "alg:none".
    let header  = B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = B64.encode(serde_json::to_vec(&claims)?);
    let signing_input = format!("{header}.{payload}");
    let signature = sign(&signing_input, secret);   // HMAC-SHA256
    Ok(format!("{signing_input}.{signature}"))
}
```

Un detalle de seguridad importante: el header con `alg` es **fijo** y al verificar no se lee el `alg` que venga en el token. Esto previene el ataque clásico de "alg confusion" (donde un atacante cambia el algoritmo a `none` o a uno más débil).

La verificación hace la firma en sentido inverso y compara con `hmac::Mac::verify_slice`, que es **tiempo constante** (evita ataques de timing que compararían byte a byte).

### Estrategia de entrega del token

La sesión puede llegar de dos formas:
1. **Cookie httpOnly `access_token`** (SameSite=Strict): el browser la envía automáticamente en cada request. No es accesible desde JavaScript - inmune a XSS.
2. **Header `Authorization: Bearer <token>`**: para clientes de API (curl, scripts).

El extractor `AuthUser` (que implementa `FromRequestParts`) prueba ambas en orden:

```rust
impl FromRequestParts<AppState> for AuthUser {
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let token = extract_from_cookie(parts)
            .or_else(|| extract_from_header(parts))
            .ok_or_else(|| AppError::Unauthorized("No token".into()))?;
        let claims = decode_token(&token, &state.auth.jwt_secret)?;
        Ok(AuthUser { user_id: claims.sub, username: claims.username })
    }
}
```

Cualquier handler que declare `_user: AuthUser` en su firma queda automáticamente protegido - si el token no existe o es inválido, Axum devuelve 401 antes de llamar al handler.

### Delay en login fallido

```rust
if !verify_password(&body.password, &hash) {
    tokio::time::sleep(Duration::from_millis(500)).await;
    return Err(AppError::Unauthorized("Invalid credentials".into()));
}
```

500ms de delay convierte un ataque de diccionario de 1000 intentos de ~2 segundos en ~8 minutos. Sin estado compartido, sin rate-limiter externo, sin complejidad.

---

## 7. El scraper: concurrencia real con Tokio

El scraper es el componente más interesante desde el punto de vista de Tokio. Con ~1400 feeds, hacer una request a la vez sería inaceptablemente lento. La solución usa `futures::stream::buffer_unordered`:

```rust
use futures::StreamExt;

let results: Vec<_> = futures::stream::iter(sources)
    .map(|source| scrape_one(state.clone(), source))
    .buffer_unordered(SCRAPE_CONCURRENCY)   // hasta 16 en paralelo
    .collect()
    .await;
```

`buffer_unordered` mantiene hasta `N` futures en ejecución simultánea, iniciando el siguiente cuando uno termina. Es el equivalente de un semáforo de concurrencia, pero sin mutexes ni channels explícitos.

### Scrape de un feed

```rust
async fn scrape_one(state: AppState, source: Source) -> ScrapeResult {
    // 1. Fetch HTTP con timeout de 15s (configurado en el reqwest::Client)
    let body = state.http.get(&source.rss_url).send().await?.text().await?;

    // 2. Parse RSS/Atom/JSON Feed con feed-rs
    let feed = feed_rs::parser::parse(body.as_bytes())?;

    // 3. Por cada entrada: extraer título, URL, resumen (strip HTML),
    //    imagen (Media RSS → enclosure → <img> embebido), severidad
    let conn = state.db.get()?;
    let tx = conn.transaction()?;
    for entry in feed.entries {
        tx.execute("INSERT OR IGNORE INTO articles ...", params![...])?;
    }
    tx.commit()?;

    // 4. Registrar en fetch_log
    // 5. UPDATE last_fetch en sources
}
```

La extracción de imágenes prueba tres fuentes en orden de preferencia:
1. `<media:content>` o `<media:thumbnail>` (Media RSS)
2. `<enclosure>` (RSS 2.0 clásico)
3. Primer `<img>` embebido en el HTML del resumen

### Inferencia de severidad

No hay ML. Un regex simple sobre el título:

```rust
fn infer_severity(title: &str) -> &'static str {
    let t = title.to_lowercase();
    if t.contains("critical") || t.contains("0-day") || t.contains("zero-day") { "critical" }
    else if t.contains("high") || t.contains("exploit") || t.contains("rce") { "high" }
    else if t.contains("medium") || t.contains("cve-") || t.contains("vulnerability") { "medium" }
    else { "low" }
}
```

Para el propósito (colorear la barra de severidad en la card) funciona bien. Un clasificador real requeriría un modelo y datos etiquetados - complejidad desproporcionada para este proyecto.

### Scheduler

El scheduler corre en un task de Tokio separado, totalmente independiente del servidor HTTP:

```rust
pub fn spawn_background_scraper(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval * 60));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await;  // consume el tick inmediato
        loop {
            ticker.tick().await;
            run_scrape_once(state.clone()).await;
        }
    });
}
```

`MissedTickBehavior::Delay` acá hace la diferencia: si el scrape tarda más que el intervalo, el siguiente se programa **después** del scrape anterior, no de forma acumulada. Así nunca hay múltiples scrapes solapados.

El estado del scrape (`scraping: bool`, `started_at`, `last_errors`) se guarda en un `Arc<RwLock<ScrapeStatus>>` que el endpoint `GET /api/scrape/status` puede leer en cualquier momento.

---

## 8. Frontend: SPA sin framework

El frontend es un único archivo `index.html` con CSS y un `app.js` externo. Sin React, sin Vue, sin bundler. Las razones:

- El servidor sirve archivos estáticos - no hay build step.
- La lógica es lineal: fetch → render. No hay estado reactivo complejo.
- Sin `npm install`, sin `node_modules`, sin `package.json`. El Dockerfile del frontend es un `COPY html/ /usr/share/nginx/html/`.

### Arquitectura de la UI

```
state = {
    view: { type: 'all' | 'folder' | 'source' | 'starred' | 'search' },
    articles: [],
    folders: [],
    sources: [],
}

selectView(v) → loadArticles() → renderArticles()
                             → renderSidebar()
```

Todos los cambios de vista siguen el mismo patrón: actualizar `state.view`, re-fetch desde la API, re-render. Sin reactividad implícita.

### El popup de artículo

Usa `<dialog>` nativo del navegador - soporte universal en browsers modernos:

```javascript
async function openArticle(id) {
    const a = state.articles.find(x => x.id == id);
    openArticleDialog(a);           // rellena el dialog y llama a .showModal()
    if (!a.isRead) {
        a.isRead = true;
        renderArticles();           // actualiza la card inmediatamente (UI optimista)
        api(`/api/articles/${id}`, { method: 'PATCH', body: JSON.stringify({ isRead: true }) });
    }
}
```

La actualización de la card es optimista: se actualiza la UI antes de confirmar el servidor, dando sensación de respuesta inmediata. Si el PATCH falla (raro), la UI quedaría momentáneamente desincronizada - aceptable para este uso.

### nginx: proxy de `/api`

El frontend y el backend son contenedores separados. Pero el browser los ve como el **mismo origen** porque nginx hace de intermediario:

```
Browser  →  nginx:80  →  sirve /index.html, /app.js
                     →  proxy /api/* → backend:3001
```

Esto elimina CORS completamente: desde el punto de vista del browser, todo viene de la misma URL.

La variable `BACKEND_UPSTREAM` parametriza el destino del proxy. nginx usa resolución **perezosa** del DNS (variable en `proxy_pass` + `resolver`): si el backend aún no arrancó cuando el frontend levanta, nginx no crashea - simplemente devuelve 502 hasta que el backend esté disponible. En Kubernetes, donde el orden de arranque no está garantizado, esto evita un CrashLoopBackOff del frontend.

---

## 9. Los tests: qué prueban y por qué

El proyecto tiene **23 tests** que corren con `cargo test`. Ninguno necesita red, base de datos externa ni el servidor levantado - todos son rápidos y deterministas. Esta sección explica qué es cada uno y qué clase de error atraparía.

### Cómo funcionan los tests en Rust

En Rust los tests viven **en el mismo archivo** que el código que prueban, dentro de un módulo marcado con `#[cfg(test)]`:

```rust
// ... código normal del módulo ...

#[cfg(test)]
mod tests {
    use super::*;              // importa todo lo del módulo padre

    #[test]
    fn nombre_del_test() {
        assert_eq!(2 + 2, 4);  // si falla, el test falla
    }
}
```

El atributo `#[cfg(test)]` significa "compilá esto **solo** cuando corras `cargo test`". En el binario de producción, ese código ni siquiera existe - no agrega peso. `use super::*` le da al módulo de test acceso a las funciones privadas del módulo padre, así que se pueden probar funciones internas sin hacerlas públicas.

Las macros de aserción:
- `assert!(cond)` - falla si `cond` es falsa.
- `assert_eq!(a, b)` - falla si `a != b`, y muestra ambos valores.
- `assert_ne!(a, b)` - falla si `a == b`.

Cada `#[test]` corre de forma aislada; si uno hace panic, los demás siguen.

### Qué se testea y qué no

La estrategia acá es **testear la lógica pura y las decisiones difíciles**, no el framework. No se testea "¿Axum enruta bien?" (eso lo garantiza Axum) sino "¿nuestra función de severidad clasifica bien?", "¿nuestro JWT rechaza una firma falsa?", "¿nuestro parser de CSV encuentra las columnas?". Son las partes donde un error sería nuestro y silencioso.

### `src/auth.rs` — 7 tests de seguridad

Es el módulo más crítico, así que es el más testeado. Un bug acá es una vulnerabilidad.

- **`hash_y_verify_password`** - hashea una contraseña y verifica que: el hash es formato Argon2id (`$argon2id$...`), la contraseña correcta valida, y una incorrecta NO valida. Es el ciclo básico de login.
- **`dos_hashes_de_la_misma_pass_son_distintos`** - hashea la misma contraseña dos veces y confirma que los hashes son distintos (porque el salt es aleatorio) pero ambos verifican. Esto prueba que el salting funciona: si dos usuarios eligen la misma contraseña, sus hashes en la DB no se parecen, así que un atacante no puede detectarlo.
- **`jwt_roundtrip`** - crea un token para el usuario 42 y lo decodifica; confirma que los datos vuelven intactos (`sub == 42`, `username == "alice"`). Prueba que firmar y verificar son inversos.
- **`jwt_firma_invalida_falla`** - crea un token con un secreto y trata de verificarlo con **otro** secreto; debe fallar. Esto es lo que impide que alguien forje un token sin conocer el `JWT_SECRET`.
- **`jwt_expirado_falla`** - crea un token con TTL de 0 segundos y confirma que se rechaza. Prueba que la expiración se respeta.
- **`jwt_malformado_falla`** - prueba varias cadenas que no son JWT válidos (`"no-es-un-jwt"`, `"a.b"`, `"a.b.c.d"`) y confirma que todas se rechazan sin panic. Prueba que el parser es robusto ante basura.
- **`random_hex_longitud_correcta`** - confirma que `random_hex(16)` devuelve 32 caracteres hex y que dos llamadas dan resultados distintos. Prueba el generador aleatorio usado para secretos.

Juntos, estos tests documentan el contrato de seguridad: *contraseñas correctamente salteadas, tokens infalsificables, expiración honrada, parser robusto*.

### `src/scraper.rs` — 7 tests de procesamiento de feeds

Prueban las funciones puras que transforman el contenido crudo de un feed en datos limpios.

- **`severidad_critica_por_keywords`** - confirma que títulos con "Critical", "Zero-Day", "ransomware", "actively exploited" se clasifican como `critical`.
- **`severidad_alta_media_baja`** - confirma la gradación: "CVE" → high, "advisory" → medium, "newsletter" → low. Estos dos tests fijan el comportamiento del clasificador por keywords; si alguien cambia un regex y rompe la clasificación, saltan.
- **`read_time_minimo_dos_minutos`** - un texto corto (o vacío) siempre da al menos "2 MIN READ". Prueba el piso.
- **`read_time_escala_con_longitud`** - un texto largo da "3 MIN READ". Prueba que el cálculo escala con el largo.
- **`strip_html_quita_tags`** - confirma que `<b>Hello</b> <i>world</i>` queda como texto sin `<` ni `>`. Prueba la limpieza de HTML.
- **`plain_summary_limpia_html_y_entidades`** - el caso completo: `<p>Hola &amp; chau <b>mundo</b></p>` → `Hola & chau mundo`. Prueba que se quitan tags Y se decodifican entidades HTML (`&amp;` → `&`).
- **`plain_summary_colapsa_espacios_y_recorta`** - confirma que un resumen larguísimo se recorta a ~280 caracteres y termina en elipsis (`…`). Prueba que las cards no reciben texto sin límite.

Estos tests son valiosos porque los feeds RSS del mundo real vienen con HTML sucio, entidades raras y longitudes impredecibles - son exactamente los casos que rompen un parser ingenuo.

### `src/importer.rs` — 6 tests de import OPML/CSV

El import acepta archivos que suben los usuarios, así que tiene que tolerar formatos variados.

- **`csv_detecta_columnas_por_nombre_sin_importar_orden`** - un CSV con columnas en cualquier orden se parsea bien mientras los nombres se reconozcan. Prueba la detección flexible de columnas.
- **`csv_maneja_bom_utf8_en_el_primer_header`** - algunos programas (Excel de Windows) ponen un carácter invisible BOM al inicio del archivo. Este test confirma que eso no rompe la detección de la primera columna. Es un caso real que arruina muchos parsers.
- **`csv_sin_columna_url_reporta_error`** - si falta la columna obligatoria (la URL del feed), se reporta un error en vez de importar basura.
- **`csv_usa_host_como_nombre_si_falta_title`** - si una fila no tiene nombre, se usa el host de la URL (`example.com`). Prueba el fallback razonable.
- **`opml_respeta_carpetas_anidadas_y_feeds_sueltos`** - un OPML con feeds dentro de carpetas y feeds sueltos: los primeros heredan la carpeta, los segundos van a "Imported". Prueba el parseo de la jerarquía.
- **`opml_maneja_outline_feed_con_hijos`** - un caso raro de OPML donde un elemento es feed Y contenedor a la vez; confirma que ambos se extraen.

Estos tests capturan el conocimiento de "cómo son los archivos OPML/CSV reales" - cada uno corresponde a un formato que alguien realmente exporta.

### `src/db.rs` — 3 tests de coherencia schema/queries

Estos se agregaron después de encontrar un bug donde una query usaba una columna que ya no existía en el schema (ver sección 12). Crean el schema en una base **en memoria** (`:memory:`, instantánea, sin tocar disco) y validan las queries contra él.

- **`article_select_is_valid_against_schema`** - prepara el `SELECT` de artículos contra el schema real. `prepare()` en SQLite falla si alguna columna o tabla no existe, así que este test atrapa cualquier desalineación entre la query y el schema. *Este es el test que habría evitado el bug de `summary_text`.*
- **`source_select_is_valid_against_schema`** - lo mismo para el SELECT de fuentes, incluido su subquery `COUNT(*)` para contar artículos.
- **`article_insert_matches_schema`** - inserta un artículo con las mismas columnas que usa el scraper y lo lee de vuelta, confirmando que el INSERT y el schema coinciden.

La técnica de la base en memoria es útil: da una DB real (con las mismas reglas de SQLite) que se crea y destruye en microsegundos, sin archivos ni limpieza.

### Correr los tests

```bash
cargo test                    # todos
cargo test auth               # solo los que tienen "auth" en el nombre
cargo test jwt_expirado       # uno específico
cargo test -- --nocapture     # mostrando los println! (para debug)
```

En el CI (`.forgejo/workflows`), aunque el paso principal es el build de las imágenes, los tests corren como parte de `cargo build --release` en el sentido de que un test que no compila rompe el build. Para correrlos explícitamente en CI se agregaría un `cargo test` antes del build.

### Qué NO está testeado (todavía)

Ver la sección 12: falta un test de integración que levante el servidor completo y haga un scrape real contra un feed de prueba. Los tests actuales son todos unitarios (funciones aisladas). Un test end-to-end atraparía errores en la interacción entre componentes, que los unitarios no ven.

---

## 10. Docker: dos contenedores bien separados

```yaml
# docker-compose.yml (simplificado)
services:
  backend:
    build: .                        # Dockerfile en la raíz
    image: azkintun-backend:latest
    volumes: [azkintun-data:/app/data]  # named volume: la SQLite vive aquí
    expose: [3001]                  # solo accesible desde la red interna
    healthcheck:
      test: curl -f http://localhost:3001/api/health

  frontend:
    build: ./frontend               # Dockerfile en frontend/
    image: azkintun-frontend:latest
    ports: ["8080:80"]              # único puerto expuesto al host
    depends_on:
      backend: {condition: service_healthy}
    environment:
      - BACKEND_UPSTREAM=backend:3001
      - DNS_RESOLVER=127.0.0.11     # DNS embebido de Docker
```

El `expose: [3001]` sin `ports:` hace que el backend no sea accesible desde el host - solo desde la red interna de Docker. La única forma de llegar al backend es a través del frontend nginx.

El `depends_on: service_healthy` garantiza que el frontend solo arranca cuando el backend responde `/api/health`. Esto evita errores de proxy en el arranque inicial de Docker Compose.

### Build del backend (multi-stage)

```dockerfile
FROM rust:1-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./        # copiar el lock antes del src
RUN cargo fetch                      # cachear dependencias
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN groupadd -g 1000 azkintun && useradd -u 1000 -g 1000 azkintun
COPY --from=builder /app/target/release/azkintun /usr/local/bin/azkintun
RUN mkdir -p /app/data && chown -R azkintun /app/data
COPY docker-entrypoint.sh /usr/local/bin/
ENTRYPOINT ["docker-entrypoint.sh"]   # ajusta permisos del volumen y baja a uid 1000
CMD ["azkintun"]
```

El multi-stage build ahorra un montón de peso: el stage de build tiene el compilador Rust completo (~1.5GB); la imagen final solo tiene el binario y `libssl`. Resultado: imagen de ~60MB en lugar de >1GB.

Copiar `Cargo.toml` y `Cargo.lock` antes del `src/` aprovecha el caché de capas de Docker: si solo cambia el código fuente (no las dependencias), Docker reutiliza la capa del `cargo fetch` y el build incremental es mucho más rápido.

### El problema del volumen y el usuario no-root

Un detalle sutil pero que rompe el arranque si no se maneja: el contenedor corre como uid 1000 (no root, por seguridad), pero cuando Docker monta un volumen sobre `/app/data`, ese punto de montaje puede quedar como root y tapar el `chown` hecho en el build. El proceso no-root no podría crear la base → `unable to open database file`.

La solución es un **entrypoint** que corre primero como root, ajusta los permisos del volumen, y luego baja privilegios a uid 1000 con `gosu`:

```sh
if [ "$(id -u)" = "0" ]; then          # Docker Compose: arrancamos como root
    chown -R azkintun:azkintun "$DATA_DIR"
    exec gosu azkintun "$@"             # baja a uid 1000 y ejecuta
else                                    # Kubernetes: ya somos uid 1000
    exec "$@"                           # (el fsGroup del PVC ya dio permisos)
fi
```

Este patrón es idiomático en imágenes Docker que necesitan escribir en volúmenes como usuario no-root. En Kubernetes no hace falta porque el `securityContext.fsGroup` resuelve los permisos del PVC de otra forma - por eso el entrypoint detecta si ya es no-root y saltea el `chown`.

---

## 11. Deploy en k3s

Esta sección explica los conceptos de k8s que aparecen en los manifiestos de la app, con el razonamiento detrás de cada decisión.

### El cluster: k3s en una Raspberry Pi

k3s es una distribución de Kubernetes ligera, diseñada para edge y homelab. Incluye:
- **Traefik** como Ingress Controller (maneja el tráfico entrante HTTP/HTTPS)
- **local-path** como StorageClass por defecto (PVCs en el filesystem local)
- **CoreDNS** para resolución de nombres dentro del cluster
- **MetalLB** (instalado aparte) para asignar IPs LAN a los Services de tipo LoadBalancer

### Por qué `nodeSelector: arm64`

```yaml
nodeSelector:
  kubernetes.io/arch: arm64
```

El cluster corre sobre Raspberry Pi (CPU ARM64). Las imágenes se construyen para esa arquitectura, así que el `nodeSelector` le dice a Kubernetes "schedulea este pod solo en nodos arm64". En un cluster de una sola arquitectura esto parece redundante, pero es una buena práctica: documenta la dependencia y previene errores si mañana se agrega un nodo x86 al cluster (el pod no intentaría correr una imagen arm64 en un nodo amd64, que fallaría con `exec format error`).

Hay dos formas de manejar arquitecturas mixtas:
- **Imágenes de una arquitectura + nodeSelector** (lo que hace este proyecto): más simple, la imagen es más chica, y el scheduling es explícito.
- **Imágenes multiarch con `docker buildx`** (manifest lists): la misma etiqueta de imagen contiene variantes para varias arquitecturas y Kubernetes elige la correcta por nodo. Más flexible pero el build es más complejo y lento.

Para un homelab de una sola plataforma, la primera opción es la pragmática. Para migrar a x86 se cambia el `nodeSelector` a `amd64` y se reconstruyen las imágenes con `--platform linux/amd64`.

### Recursos en `k8s/manifests.yaml`

```
Namespace azkintun
    ├── Secret azkintun-secret          (JWT_SECRET + credenciales admin)
    ├── PersistentVolumeClaim           (la SQLite vive acá)
    ├── Deployment azkintun-backend     (el binario Rust)
    │       └── Service azkintun-backend  (ClusterIP, solo interno)
    ├── Deployment azkintun-frontend    (nginx)
    │       └── Service azkintun-frontend (ClusterIP → expuesto por Ingress)
    ├── Ingress azkintun                (Traefik, TLS, redirect HTTP→HTTPS)
    └── Middleware redirect-https       (CRD de Traefik)
```

### Por qué `strategy: Recreate`

```yaml
strategy:
  type: Recreate    # ← no RollingUpdate
```

El backend usa SQLite con un PVC `ReadWriteOnce` (solo un pod puede montar el volumen a la vez). Con la estrategia por defecto `RollingUpdate`, Kubernetes levantaría el pod nuevo **antes** de bajar el viejo - el nuevo no podría montar el PVC porque el viejo lo tiene. Con `Recreate`, el viejo baja primero y luego sube el nuevo. La app tiene unos segundos de downtime al actualizar - aceptable para un lector RSS personal.

Si necesitaras zero-downtime, la solución sería migrar a Postgres (que soporta múltiples conexiones concurrentes de múltiples pods).

### Por qué el Secret no va en Git

El manifiesto incluye un bloque `Secret` con el valor `REEMPLAZAR_...` como placeholder. El `deploy.sh` **filtra ese bloque** y crea el Secret real en el cluster con valores generados aleatoriamente:

```bash
jwt=$(head -c 48 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 48)
kubectl create secret generic azkintun-secret \
  --from-literal=JWT_SECRET=$jwt \
  ...
```

El Secret existe en el cluster (etcd, en memoria) pero nunca en el repositorio. Si el repo se hace público, no hay filtración de credenciales.

### PersistentVolumeClaim: el backup es un archivo

```yaml
kind: PersistentVolumeClaim
spec:
  storageClassName: local-path    # usa el disco local del nodo
  resources:
    requests: {storage: 2Gi}
```

Con `local-path`, k3s crea un directorio en el filesystem del nodo (habitualmente bajo `/var/lib/rancher/k3s/storage/`). El "volumen" es una carpeta real. Para hacer backup:

```bash
kubectl exec -n azkintun deploy/azkintun-backend -- \
  cat /app/data/azkintun.db > backup-$(date +%F).db
```

Un archivo SQLite de backup es auto-contenido y restaurable en cualquier instancia sin herramientas especiales.

### Ingress: cómo llega el tráfico desde LAN

```
Usuario LAN  →  IP Traefik (MetalLB)  →  Traefik  →  Ingress azkintun
                192.168.2.200:443                      host: azkintun.local
                                                            ↓
                                                   Service azkintun-frontend
                                                            ↓
                                                   Pod nginx (proxy /api)
                                                            ↓
                                                   Service azkintun-backend
```

El TLS termina en Traefik. Detrás de Traefik, todo el tráfico interno al cluster es HTTP plano. Por eso el backend usa `COOKIE_SECURE=false` - la cookie `access_token` no necesita el flag Secure porque el navegador ya habló HTTPS con Traefik.

### Redirect HTTP → HTTPS

```yaml
# Middleware (CRD de Traefik)
kind: Middleware
metadata: {name: redirect-https}
spec:
  redirectScheme: {scheme: https, permanent: true}

# Ingress
annotations:
  traefik.ingress.kubernetes.io/router.middlewares: "azkintun-redirect-https@kubernetescrd"
```

Traefik aplica el middleware antes de hacer el proxy. El `permanent: true` devuelve un 301 (el browser lo recuerda en caché). El nombre de la annotation sigue el formato `namespace-nombre@kubernetescrd` - un detalle de Traefik que quema horas si se escribe mal.

### Variables de publicación sin editar el YAML

El `deploy.sh` acepta tres variables de entorno que parametrizan el Ingress sin tocar el manifiesto. (En los ejemplos se abrevia como `deploy.sh`; el path real desde la raíz del repo es `k8s/3000-apps/azkintun/deploy.sh`.)

```bash
# LAN con self-signed (default)
AZKINTUN_HOST=azkintun.local bash k8s/3000-apps/azkintun/deploy.sh

# Dominio público con cert válido de Let's Encrypt
AZKINTUN_HOST=rss.midominio.com AZKINTUN_ISSUER=letsencrypt-prod \
  bash k8s/3000-apps/azkintun/deploy.sh

# Solo HTTP, sin TLS (pruebas o proxy externo)
AZKINTUN_TLS=false bash k8s/3000-apps/azkintun/deploy.sh
```

El script hace `sed` sobre los valores por defecto del manifiesto y, en el caso `TLS=false`, usa Python para parsear y modificar el YAML (quitar el bloque `tls:`, el Middleware, etc.). Esto funciona con el manifiesto "como viene" desde el repositorio - los defaults son valores reales, no placeholders.

### Resolución DNS perezosa del upstream

Este es un detalle específico de Kubernetes que suele sorprender. En la config de nginx del frontend:

```nginx
resolver ${DNS_RESOLVER} valid=10s ipv6=off;   # CoreDNS del cluster
location /api/ {
    set $backend http://${BACKEND_UPSTREAM};    # variable, no upstream{}
    proxy_pass $backend;
}
```

Con un bloque `upstream { server azkintun-backend:3001; }` estático, nginx resuelve el DNS **al arrancar**. Si el pod del backend aún no existe cuando el frontend arranca, nginx no puede resolver el nombre y el contenedor entra en error.

Con `set $backend ...` y `proxy_pass $backend`, nginx resuelve el nombre **en cada request**. El frontend arranca siempre; si el backend no está, devuelve 502 hasta que aparece. En un cluster k8s, donde el orden de arranque no está garantizado, esto es lo que evita que el frontend entre en CrashLoopBackOff esperando al backend.

### CI/CD con Forgejo Actions

El workflow `.forgejo/workflows/build-and-deploy.yml` implementa el ciclo completo:

```
push a main
    → build imagen backend (cargo build --release)
    → build imagen frontend (nginx + SPA)
    → smoke test: docker run + curl /api/health
    → importar imágenes a k3s (k3s ctr images import)
    → kubectl apply (sin Secret, con host/issuer de Variables del repo)
    → kubectl rollout status (esperar hasta que esté listo)
```

El Secret **nunca pasa por el CI**. Una vez creado en el cluster por el `deploy.sh` inicial, el CI solo actualiza código - el Secret persiste en etcd independientemente.

Las Variables del repo (`AZKINTUN_HOST`, `AZKINTUN_ISSUER`, `AZKINTUN_TLS`) en Forgejo → Settings → Actions → Variables permiten que el CI use el mismo host que el deploy manual, sin hardcodear nada en el workflow.

---

## 12. Observaciones y estado

### Corregidas

Estas observaciones surgieron durante la revisión y ya están resueltas en el código:

**`summary_text` en `ARTICLE_SELECT` (crítica).** La query de `routes/articles.rs` referenciaba `a.summary_text`, una columna que había sido eliminada del schema (era un duplicado de `summary`). En una instancia nueva, el endpoint de artículos fallaba en runtime con "no such column". Corregido a `a.summary`. Además se agregó un **test de regresión** (`db::tests::article_select_is_valid_against_schema`) que prepara el SQL contra el schema real y falla si vuelve a aparecer una columna inexistente. Lo mismo para el SELECT de fuentes y el INSERT del scraper.

**Detección de entorno frágil en `db.rs`.** El fallback que adivinaba `/app/data` mirando si el path existía o si había una variable `DOCKER` se eliminó. Ahora `db_path()` usa `AZKINTUN_DATA_DIR` con un fallback simple al directorio actual. La variable está definida en el Dockerfile (`/app/data`), en el manifiesto k8s y documentada en `.env.example`, así que no hay ambigüedad.

**`seeds.sql` podía quedar desactualizado.** Se agregó `scripts/generate-seeds-sql.sh` que regenera el dump desde una DB recién sembrada por el binario. Tiene un modo `--check` que falla si `seeds.sql` no está al día respecto a `src/seeds_data.rs`, y el CI lo ejecuta en cada push.

**Sin caché de capas Docker en el CI.** El workflow ahora habilita BuildKit (`DOCKER_BUILDKIT=1`) y pasa `BUILDKIT_INLINE_CACHE=1` a los builds, para reutilizar capas entre corridas. (El Dockerfile del backend ya cacheaba las dependencias de Rust por separado del código, con el patrón del `main.rs` placeholder.)

### Pendientes (sin urgencia)

**Falta test de integración end-to-end del scraper.** Los tests cubren lógica pura (hashing, JWT, CSV/OPML, severidad, validez de queries contra el schema). No hay uno que levante el servidor completo y haga un scrape real contra un feed de prueba servido localmente. Eso dejaría sin detectar regresiones en la extracción de imágenes o el dedup por URL. Requeriría un mock HTTP server (p. ej. `wiremock`) o un fixture de feed local.

**`SCRAPE_CONCURRENCY` no tiene backpressure global.** Con 16 requests concurrentes hay ráfagas cada vez que termina un batch. No hay límite de requests por segundo sobre el total. La mayoría de las fuentes lo toleran, pero algunas podrían devolver 429. Una mejora sería un pequeño delay aleatorio por request, o un rate-limiter con token bucket.

**El scheduler no persiste el historial de scrapes.** El estado (`last_finished_at`, `last_errors`) vive solo en memoria (`Arc<RwLock<ScrapeStatus>>`). Tras un reinicio se pierde. Para un dashboard de salud histórico habría que persistirlo - pero `fetch_log` ya guarda lo esencial por fuente, así que el gap es menor.

---

*Este instructivo se corresponde con el estado del código en el repositorio `azkintun` tras la ronda de correcciones. Las observaciones pendientes son conocidas y no afectan el funcionamiento normal.*
