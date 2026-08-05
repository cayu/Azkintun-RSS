# Azkintun-RSS

Agregador de noticias de ciberseguridad con **carpetas estilo Inoreader**,
reescrito en Rust (Axum + SQLite embebido) a partir del proyecto original
en TypeScript/Express.

Incluye **autenticación** (Argon2id + JWT) y una arquitectura de **dos
contenedores** (backend + frontend/nginx) pensada para exponer solo el
frontend y mantener el backend aislado en la red interna de Docker.

> **Documentación relacionada:**
> - [`INSTRUCTIVO.md`](INSTRUCTIVO.md) - recorrido didáctico de la arquitectura,
>   el código Rust, los tests y el deploy (pensado como material de estudio).
> - [`k8s/3000-apps/azkintun/README.md`](k8s/3000-apps/azkintun/README.md) —
>   guía de despliegue en Kubernetes/k3s.

## Arquitectura

```
   navegador
       │  http(s)://localhost:8080
       ▼
┌──────────────────┐   red interna Docker    ┌──────────────────┐
│  frontend        │  ────────────────────►  │  backend         │
│  (nginx)         │      /api  →  :3001      │  (axum + SQLite) │
│  sirve el SPA    │                          │  NO expone puerto│
│  proxy de /api   │                          │  al host         │
└──────────────────┘                          └──────────────────┘
   único puerto                                  volumen azkintun-data
   publicado (:8080)
```

El navegador solo habla con nginx (mismo origen) → sin CORS. El backend
nunca queda expuesto directamente: solo el frontend puede alcanzarlo por
la red interna.

## Quick start

```bash
cp .env.example .env        # editá JWT_SECRET y ADMIN_PASSWORD
docker compose up -d --build
# abrí http://localhost:8080
docker compose logs -f
```

La primera vez, el backend:

1. Crea el schema SQLite en el volumen `azkintun-data` (montado en `/app/data`).
2. Siembra ~1400 fuentes RSS de ciberseguridad (las del proyecto
   original más las colecciones de Inoreader y CyberSecurityRSS),
   organizadas en 30 categorías.
3. Crea el usuario admin (según `.env`) y dispara un scrape inicial;
   luego re-scrapea cada `SCRAPE_INTERVAL_MINUTES`.

Si no definís `ADMIN_PASSWORD`, se genera una aleatoria y se imprime una
vez en los logs (`docker compose logs backend | grep AUTH`).

> **Persistencia.** La base SQLite vive en un named volume de Docker
> (`azkintun-data`), no en un directorio del host. El contenedor corre como
> usuario no-root (uid 1000) y un entrypoint ajusta los permisos del volumen
> automáticamente al arrancar - no hace falta ningún `chown` manual. Para
> sacar una copia de la base: `docker compose cp backend:/app/data/azkintun.db .`
> (o usá el botón Exportar de la app). Para borrar los datos:
> `docker compose down -v`.

## Deploy en Kubernetes (k3s / homelab)

Además de Docker Compose, el repo incluye manifiestos y scripts para
desplegar en un cluster k3s, siguiendo la convención del homelab
`k3s-pi5-forgejo` (apps bajo `k8s/3000-apps/<app>/`):

```bash
bash k8s/3000-apps/azkintun/deploy.sh
```

Construye las dos imágenes (backend Rust + frontend nginx) para la
arquitectura del cluster, las importa a containerd de k3s, genera un
`JWT_SECRET` y un `ADMIN_PASSWORD` aleatorios en un Secret, y aplica los
manifiestos (namespace, PVC para la SQLite, ambos Deployments, Services e
Ingress con TLS). Detalle completo en
[`k8s/3000-apps/azkintun/README.md`](k8s/3000-apps/azkintun/README.md).

La misma imagen de frontend sirve en Compose y en k8s: el upstream del
reverse-proxy `/api` se parametriza con `BACKEND_UPSTREAM` (default
`backend:3001` para Compose; `azkintun-backend:3001` en k8s).

## Autenticación

Todos los endpoints requieren autenticación **excepto** `GET /api/health`
(healthcheck) y `POST /api/auth/login`.

- **Contraseñas**: hasheadas con **Argon2id** (salt aleatorio, parámetros
  fuertes por defecto). Nunca se guardan en texto plano.
- **Sesiones**: **JWT firmado con HMAC-SHA256**. El token viaja de dos
  formas y el backend acepta cualquiera:
  - **Cookie** `access_token` - `HttpOnly; SameSite=Strict; Secure`
    (recomendado para el navegador; inmune a robo por XSS, y SameSite
    corta CSRF). La usa el frontend automáticamente.
  - **Header** `Authorization: Bearer <token>` - para clientes de API,
    scripts o mobile.

### Flujo

```bash
# login → devuelve el token y setea la cookie httpOnly
curl -c cookies.txt -X POST http://localhost:8080/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"<tu-password>"}'

# usar la cookie
curl -b cookies.txt http://localhost:8080/api/folders

# o usar el token como Bearer
TOKEN=... # el campo "token" de la respuesta de login
curl http://localhost:8080/api/folders -H "Authorization: Bearer $TOKEN"

# cambiar contraseña
curl -b cookies.txt -X POST http://localhost:8080/api/auth/change-password \
  -H 'Content-Type: application/json' \
  -d '{"currentPassword":"...","newPassword":"..."}'
```

### Endpoints de auth

```
POST /api/auth/login             { username, password } → { token, expiresIn, user }
POST /api/auth/logout            expira la cookie
GET  /api/auth/me                usuario actual
POST /api/auth/change-password   { currentPassword, newPassword }
```

## Variables de entorno

| Variable                  | Default | Descripción                                              |
|----------------------------|---------|-----------------------------------------------------------|
| `JWT_SECRET`               | (gen.)  | Secreto para firmar JWT. **>= 32 chars.** Si falta, se genera uno temporal (los tokens no sobreviven reinicios). |
| `ADMIN_USERNAME`           | `admin` | Usuario admin inicial (solo al crear la DB)               |
| `ADMIN_PASSWORD`           | (gen.)  | Password del admin inicial; si falta, se genera y se loguea |
| `JWT_TTL_HOURS`            | `24`    | Vida útil de un token de sesión                           |
| `COOKIE_SECURE`            | `true`  | Flag `Secure` de la cookie. **`false` para http local**, `true` solo detrás de HTTPS |
| `CORS_ALLOWED_ORIGIN`      | (vacío) | Origen permitido para CORS (solo si el frontend corre en otro origen; con el proxy nginx no hace falta) |
| `PORT`                     | `3001`  | Puerto HTTP del backend (interno, no se expone al host)   |
| `FRONTEND_PORT`            | `8080`  | Puerto del host por el que se accede a la app (solo Docker Compose) |
| `SCRAPE_INTERVAL_MINUTES`  | `15`    | Frecuencia del scrape periódico                           |
| `SCRAPE_CONCURRENCY`       | `16`    | Fuentes scrapeadas en paralelo (importante con ~1400 feeds) |
| `SCRAPE_ON_STARTUP`        | `true`  | Si es `0`/`false`, no scrapea al arrancar                 |
| `AZKINTUN_DATA_DIR`      | `/app/data` en Docker | Dónde vive `azkintun.db`                  |
| `RUST_LOG`                 | `info`  | Nivel de logging                                          |

### Cambiar el puerto de la app

Por defecto la app se publica en el puerto **8080** del host. Si ya tenés
algo corriendo ahí, cambiá `FRONTEND_PORT` (solo afecta al frontend; el
backend nunca se expone al host).

En el `.env`:

```
FRONTEND_PORT=9090
```

```bash
docker compose up -d
# ahora abrí http://localhost:9090
```

O sin editar el `.env`, pasándolo en la misma línea:

```bash
FRONTEND_PORT=9090 docker compose up -d
```

Cualquier puerto libre sirve. En Kubernetes esto no aplica: el acceso va
por el Ingress (puertos 80/443) o por `port-forward`, donde elegís el
puerto local:

```bash
kubectl port-forward -n azkintun svc/azkintun-frontend 9090:80
```

### HTTPS en producción

`COOKIE_SECURE=true` requiere que el frontend se sirva por HTTPS (si no,
el navegador descarta la cookie). Para producción, terminá TLS en nginx
(agregando un `listen 443 ssl` con tus certificados en `frontend/nginx.conf.template`)
o poné un reverse-proxy/con TLS por delante, y recién ahí activá
`COOKIE_SECURE=true`.

## Carpetas y fuentes

- **Carpetas** son entidades reales (no un string suelto): tienen CRUD y
  cuentan artículos no leídos.
- Cada **fuente** RSS pertenece opcionalmente a una carpeta
  (`folderId: null` = sin carpeta). Al actualizar, `folderId: 0` = sacar
  de cualquier carpeta.

## Import de suscripciones

### OPML (recomendado)

Formato nativo de export/import de Inoreader (y de casi cualquier lector
RSS). Respeta las carpetas del OPML.

```bash
curl -b cookies.txt -X POST http://localhost:8080/api/import/opml -F "file=@subscriptions.opml"
```

### CSV

Detecta columnas por nombre (sin importar el orden), en inglés y español,
y maneja BOM UTF-8:

| Contenido       | Nombres de columna aceptados                                   |
|------------------|-----------------------------------------------------------------|
| URL (**obligatoria**) | `url`, `feed url`, `rss url`, `xmlUrl`, `feedUrl`, `link`   |
| Nombre           | `title`, `name`, `feed title`, `nombre` (si falta → host de la URL) |
| Carpeta          | `folder`, `category`, `categoria`, `carpeta`, `tags` (si falta → "Imported") |

```bash
curl -b cookies.txt -X POST http://localhost:8080/api/import/csv -F "file=@feeds.csv"
```

Ambos son idempotentes: una fuente ya existente (mismo `rss_url`) se
cuenta como `sourcesSkipped`.

## Export de suscripciones

Descargá todas tus fuentes en formato estándar para respaldarlas o
migrarlas a otro lector:

```bash
# OPML — importable en Inoreader, Feedly, Miniflux, etc.
curl -b cookies.txt http://localhost:8080/api/export/opml -o subscriptions.opml

# CSV — para hojas de cálculo o para reimportar en Azkintun-RSS
curl -b cookies.txt http://localhost:8080/api/export/csv -o subscriptions.csv
```

En la UI, el botón **Exportar** del sidebar abre un selector de formato y
dispara la descarga. El OPML agrupa los feeds por carpeta; el CSV tiene
las columnas `Name, Feed URL, Folder`.

## Endpoints principales

```
GET    /api/health                        (público) healthcheck

POST   /api/auth/login                    (público)
POST   /api/auth/logout | GET /api/auth/me | POST /api/auth/change-password

GET    /api/stats

GET    /api/folders | POST /api/folders
PATCH  /api/folders/:id | DELETE /api/folders/:id
POST   /api/folders/:id/mark-all-read

GET    /api/sources?folderId= | POST /api/sources
PATCH  /api/sources/:id | DELETE /api/sources/:id

GET    /api/articles?folderId=&sourceId=&search=&unreadOnly=&starredOnly=&limit=&offset=
GET    /api/articles/:id | PATCH /api/articles/:id     { isRead?, isStarred? }
POST   /api/articles/mark-all-read?folderId=&sourceId=

POST   /api/import/csv | POST /api/import/opml         (multipart, campo "file")
GET    /api/export/opml | GET /api/export/csv          (descarga con Content-Disposition)

POST   /api/scrape | GET /api/scrape/status
```

## Frontend

`frontend/` es el contenedor nginx: sirve `frontend/html/` (`index.html`
+ `app.js`) y proxea `/api` al backend.

El frontend es **HTML + JavaScript vanilla** (dos archivos, sin build, sin
npm, sin framework). El JS está en `app.js` aparte para cumplir con la
Content-Security-Policy (`script-src 'self'`, sin scripts inline). Cubre
toda la funcionalidad:

- Login / logout / cambio de contraseña
- Sidebar con **categorías** colapsables y sus feeds, con contador de no
  leídos por categoría
- Vistas rápidas: Todos / No leídos / Favoritos
- Lista de artículos estilo Inoreader: **imagen destacada** (thumbnail),
  **resumen** en texto plano, badge de severidad y fecha relativa; al
  abrir muestra la imagen grande y el resumen completo, y se marca leído
- **Guardar en favoritos** (estrella) con filtro dedicado
- "Marcar todo leído"
- Gestión de categorías: crear, renombrar, eliminar
- Gestión de feeds: agregar (asignando categoría), mover entre
  categorías, activar/desactivar, eliminar
- Importar suscripciones (OPML/CSV)
- Actualizar feeds manualmente (con estado en vivo)

Las imágenes de los artículos se extraen del RSS (Media RSS, enclosures o
`<img>` embebida) y se muestran desde su origen (el CSP permite `https:`
en `img-src`). Usa la cookie httpOnly (mismo origen vía el proxy), así
que el token nunca se toca desde JavaScript.

Para reemplazarlo por un SPA con build (React/Vite u otro), agregar un
stage de Node en `frontend/Dockerfile` (ya está comentado cómo) y servir
`dist/` en lugar de `html/`. El proxy y la seguridad no cambian.

## Desarrollo local (sin Docker)

Requiere Rust (1.75+ con los pines del `Cargo.toml`, o cualquier
toolchain moderno).

```bash
JWT_SECRET=$(openssl rand -hex 32) ADMIN_PASSWORD=changeme COOKIE_SECURE=false cargo run
```

La base SQLite se crea en `./azkintun.db`.

### Tests

```bash
cargo test
```

Cubre la lógica pura crítica: hashing y verificación de contraseñas,
round-trip / expiración / firma de JWT, detección de columnas del CSV
(con BOM), parseo de OPML anidado, inferencia de severidad y tiempo de
lectura.

### `seeds.sql` — dump de referencia

En la raíz hay un `seeds.sql` con el schema completo más las carpetas y
fuentes por defecto (sin artículos ni usuarios). Sirve como referencia de
la estructura de datos y para levantar una DB limpia sin arrancar el
binario:

```bash
sqlite3 nueva.db < seeds.sql
```

Se regenera desde una DB recién sembrada; no se edita a mano. Para
regenerarlo tras cambiar `src/seeds_data.rs`:

```bash
bash scripts/generate-seeds-sql.sh          # regenera seeds.sql
bash scripts/generate-seeds-sql.sh --check  # falla si está desactualizado (lo usa el CI)
```

### Nota sobre versiones fijas en `Cargo.toml`

Varias dependencias fijadas a versiones exactas porque
el proyecto se desarrolló/probó contra un Rust 1.75.
Se dejaron fijas por ser la configuración exacta que se compiló y probó.
