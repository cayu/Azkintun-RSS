# Azkintun-RSS — Referencia de API

La API es REST sobre HTTP/HTTPS. El base URL es la raíz del servidor; en
desarrollo local `http://localhost:3001`, en producción el dominio que tengas
configurado (`https://azkintun.tudominio.com`). Todos los cuerpos son JSON
salvo los endpoints de import/export.

---

## Autenticación

La API soporta dos mecanismos en paralelo:

**Cookie httpOnly** (para browsers): el login setea una cookie `access_token`
(HttpOnly, SameSite=Strict). El browser la envía automáticamente en cada
request. No es accesible desde JS — protege contra XSS.

**Header Bearer** (para clientes de API): el token también viene en el body
del login. Se envía como `Authorization: Bearer <token>`.

El token es un JWT HS256 con TTL configurable (default 24h). Todos los
endpoints salvo `/api/health` y `/api/auth/login` requieren autenticación.
Sin token válido se recibe `401 Unauthorized`.

---

## Errores

Todos los errores devuelven JSON con la forma:

```json
{ "error": "descripción del error" }
```

Códigos usados:

| Código | Significado |
|--------|-------------|
| `400`  | Bad Request — parámetro faltante o inválido |
| `401`  | Unauthorized — token ausente, expirado o inválido |
| `404`  | Not Found — el recurso no existe |
| `409`  | Conflict — ya existe (nombre/URL duplicado) |
| `500`  | Internal Server Error |

---

## Auth

### POST /api/auth/login

Inicia sesión. Devuelve el token y setea la cookie.

**Body:**
```json
{
  "username": "admin",
  "password": "tu-contraseña"
}
```

**Respuesta 200:**
```json
{
  "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "expiresIn": 86400,
  "user": {
    "id": 1,
    "username": "admin"
  }
}
```

```bash
curl -c cookies.txt -X POST https://azkintun.tudominio.com/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"tu-contraseña"}'
```

Con Bearer token en lugar de cookies:
```bash
TOKEN=$(curl -s -X POST https://azkintun.tudominio.com/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"tu-contraseña"}' \
  | jq -r .token)

# Usar en requests siguientes:
curl -H "Authorization: Bearer $TOKEN" https://azkintun.tudominio.com/api/stats
```

---

### GET /api/auth/me

Devuelve el usuario autenticado.

**Respuesta 200:**
```json
{ "id": 1, "username": "admin" }
```

```bash
curl -b cookies.txt https://azkintun.tudominio.com/api/auth/me
```

---

### POST /api/auth/logout

Expira la cookie de sesión.

**Respuesta 200:**
```json
{ "success": true }
```

```bash
curl -b cookies.txt -c cookies.txt -X POST \
  https://azkintun.tudominio.com/api/auth/logout
```

---

### POST /api/auth/change-password

Cambia la contraseña del usuario autenticado.

**Body:**
```json
{
  "currentPassword": "contraseña-actual",
  "newPassword": "nueva-contraseña-min-8-chars"
}
```

**Respuesta 200:**
```json
{ "success": true }
```

**Errores:** `400` si la nueva contraseña tiene menos de 8 caracteres.
`401` si la contraseña actual es incorrecta.

```bash
curl -b cookies.txt -X POST https://azkintun.tudominio.com/api/auth/change-password \
  -H "Content-Type: application/json" \
  -d '{"currentPassword":"vieja","newPassword":"nuevapass123"}'
```

---

## Health y Stats

### GET /api/health

Endpoint público para healthchecks. No requiere token.

**Respuesta 200:**
```json
{ "status": "ok" }
```

```bash
curl https://azkintun.tudominio.com/api/health
```

---

### GET /api/stats

Estadísticas generales de la instancia.

**Respuesta 200:**
```json
{
  "totalArticles": 48230,
  "totalSources": 1390,
  "totalFolders": 30,
  "unreadArticles": 7703,
  "lastFetch": "2026-08-04T20:15:33Z"
}
```

```bash
curl -b cookies.txt https://azkintun.tudominio.com/api/stats
```

---

## Carpetas (Folders)

### GET /api/folders

Lista todas las carpetas con contadores.

**Respuesta 200:**
```json
[
  {
    "id": 1,
    "name": "Seginfo-CVE",
    "sourceCount": 12,
    "unreadCount": 342
  },
  {
    "id": 2,
    "name": "Home Lab",
    "sourceCount": 8,
    "unreadCount": 54
  }
]
```

```bash
curl -b cookies.txt https://azkintun.tudominio.com/api/folders
```

---

### POST /api/folders

Crea una carpeta nueva.

**Body:**
```json
{ "name": "Mi nueva carpeta" }
```

**Respuesta 200:** objeto `Folder` creado.

**Errores:** `400` si el nombre está vacío. `409` si ya existe una carpeta
con ese nombre.

```bash
curl -b cookies.txt -X POST https://azkintun.tudominio.com/api/folders \
  -H "Content-Type: application/json" \
  -d '{"name":"Inteligencia Artificial"}'
```

---

### PATCH /api/folders/:id

Renombra una carpeta.

**Body:**
```json
{ "name": "Nuevo nombre" }
```

**Respuesta 200:** objeto `Folder` actualizado.

```bash
curl -b cookies.txt -X PATCH https://azkintun.tudominio.com/api/folders/1 \
  -H "Content-Type: application/json" \
  -d '{"name":"CVEs Críticos"}'
```

---

### DELETE /api/folders/:id

Borra la carpeta. Los feeds que estaban en ella quedan sin carpeta (no se
borran). Sus artículos tampoco se borran.

**Respuesta 200:**
```json
{ "success": true }
```

```bash
curl -b cookies.txt -X DELETE https://azkintun.tudominio.com/api/folders/1
```

---

### POST /api/folders/:id/mark-all-read

Marca como leídos todos los artículos de los feeds de esa carpeta.

**Respuesta 200:**
```json
{ "success": true, "updated": 342 }
```

```bash
curl -b cookies.txt -X POST \
  https://azkintun.tudominio.com/api/folders/1/mark-all-read
```

---

## Fuentes RSS (Sources)

### GET /api/sources

Lista todas las fuentes. Opcionalmente filtrá por carpeta.

**Query params:**

| Param | Tipo | Descripción |
|-------|------|-------------|
| `folderId` | int | Solo devuelve fuentes de esa carpeta |

**Respuesta 200:**
```json
[
  {
    "id": 42,
    "name": "Krebs on Security",
    "rssUrl": "https://krebsonsecurity.com/feed/",
    "folderId": 1,
    "folderName": "Seginfo-Noticias",
    "active": true,
    "custom": false,
    "articleCount": 1240,
    "lastFetch": "2026-08-04T20:00:00Z",
    "lastError": null
  }
]
```

```bash
# Todas las fuentes
curl -b cookies.txt https://azkintun.tudominio.com/api/sources

# Solo de una carpeta
curl -b cookies.txt "https://azkintun.tudominio.com/api/sources?folderId=1"
```

---

### POST /api/sources

Agrega una fuente nueva.

**Body:**
```json
{
  "name": "Dark Reading",
  "rssUrl": "https://www.darkreading.com/rss.xml",
  "folderId": 1
}
```

También podés pasar `folderName` en lugar de `folderId`. Si la carpeta con
ese nombre no existe, se crea automáticamente:

```json
{
  "name": "Dark Reading",
  "rssUrl": "https://www.darkreading.com/rss.xml",
  "folderName": "Seginfo-Noticias"
}
```

**Respuesta 200:** objeto `Source` creado.

**Errores:** `400` si faltan `name` o `rssUrl`, o si la carpeta no existe.
`409` si ya hay una fuente con esa URL.

```bash
curl -b cookies.txt -X POST https://azkintun.tudominio.com/api/sources \
  -H "Content-Type: application/json" \
  -d '{"name":"Dark Reading","rssUrl":"https://www.darkreading.com/rss.xml","folderId":1}'
```

---

### PATCH /api/sources/:id

Actualiza una fuente. Todos los campos son opcionales.

**Body (todos opcionales):**
```json
{
  "name": "Nuevo nombre",
  "active": false,
  "folderId": 2
}
```

Para quitar una fuente de cualquier carpeta (dejarla sin categoría), mandá
`"folderId": 0`.

**Respuesta 200:** objeto `Source` actualizado.

```bash
# Desactivar una fuente (deja de scrapearse)
curl -b cookies.txt -X PATCH https://azkintun.tudominio.com/api/sources/42 \
  -H "Content-Type: application/json" \
  -d '{"active":false}'

# Mover a otra carpeta
curl -b cookies.txt -X PATCH https://azkintun.tudominio.com/api/sources/42 \
  -H "Content-Type: application/json" \
  -d '{"folderId":3}'

# Quitar de carpeta
curl -b cookies.txt -X PATCH https://azkintun.tudominio.com/api/sources/42 \
  -H "Content-Type: application/json" \
  -d '{"folderId":0}'
```

---

### DELETE /api/sources/:id

Borra la fuente y todos sus artículos.

**Respuesta 200:**
```json
{ "success": true }
```

```bash
curl -b cookies.txt -X DELETE https://azkintun.tudominio.com/api/sources/42
```

---

## Artículos

### GET /api/articles

Lista artículos con filtros opcionales. Ordenados por fecha de publicación
descendente (más nuevos primero).

**Query params:**

| Param | Tipo | Descripción |
|-------|------|-------------|
| `folderId` | int | Artículos de fuentes en esa carpeta |
| `sourceId` | int | Artículos de esa fuente |
| `search` | string | Busca en título y resumen |
| `unreadOnly` | bool | Solo no leídos |
| `starredOnly` | bool | Solo favoritos |
| `limit` | int | Máximo de resultados (default 50, máx 500) |
| `offset` | int | Para paginación |

**Respuesta 200:**
```json
[
  {
    "id": 9876,
    "title": "Critical CVE-2026-1234 in OpenSSL",
    "summary": "A vulnerability classified as critical has been found...",
    "sourceId": 42,
    "sourceName": "vuldb.com",
    "folderId": 1,
    "folderName": "Seginfo-CVE",
    "severity": "critical",
    "publishedAt": "2026-08-04T18:30:00Z",
    "readTime": "2 MIN READ",
    "url": "https://vuldb.com/?id.12345",
    "imageUrl": "https://vuldb.com/img/logo.png",
    "isRead": false,
    "isStarred": false
  }
]
```

**Valores posibles de `severity`:** `critical`, `high`, `medium`, `low`.

```bash
# Últimos 100 artículos
curl -b cookies.txt "https://azkintun.tudominio.com/api/articles?limit=100"

# No leídos de una carpeta
curl -b cookies.txt \
  "https://azkintun.tudominio.com/api/articles?folderId=1&unreadOnly=true&limit=50"

# Buscar
curl -b cookies.txt \
  "https://azkintun.tudominio.com/api/articles?search=openssl&limit=20"

# Favoritos
curl -b cookies.txt \
  "https://azkintun.tudominio.com/api/articles?starredOnly=true"

# Paginación (página 2 de 50)
curl -b cookies.txt \
  "https://azkintun.tudominio.com/api/articles?limit=50&offset=50"
```

---

### GET /api/articles/:id

Devuelve un artículo por su ID.

**Respuesta 200:** objeto `Article` (misma forma que en la lista).

```bash
curl -b cookies.txt https://azkintun.tudominio.com/api/articles/9876
```

---

### PATCH /api/articles/:id

Actualiza el estado de un artículo. Campos opcionales.

**Body:**
```json
{
  "isRead": true,
  "isStarred": false
}
```

**Respuesta 200:** objeto `Article` actualizado.

```bash
# Marcar como leído
curl -b cookies.txt -X PATCH https://azkintun.tudominio.com/api/articles/9876 \
  -H "Content-Type: application/json" \
  -d '{"isRead":true}'

# Agregar a favoritos
curl -b cookies.txt -X PATCH https://azkintun.tudominio.com/api/articles/9876 \
  -H "Content-Type: application/json" \
  -d '{"isStarred":true}'
```

---

### POST /api/articles/mark-all-read

Marca como leídos todos los artículos (o los de una carpeta/fuente).

**Query params (opcionales):**

| Param | Tipo | Descripción |
|-------|------|-------------|
| `folderId` | int | Solo los artículos de esa carpeta |
| `sourceId` | int | Solo los artículos de esa fuente |

**Respuesta 200:**
```json
{ "success": true, "updated": 7703 }
```

```bash
# Marcar todos como leídos
curl -b cookies.txt -X POST \
  https://azkintun.tudominio.com/api/articles/mark-all-read

# Solo los de una carpeta
curl -b cookies.txt -X POST \
  "https://azkintun.tudominio.com/api/articles/mark-all-read?folderId=1"
```

---

## Scrape

### POST /api/scrape

Dispara un scrape manual de todos los feeds activos. La respuesta es
inmediata — el scrape corre en background. Consultá el status para saber
cuándo termina.

**Respuesta 200:**
```json
{ "success": true, "message": "Scraping started" }
```

```bash
curl -b cookies.txt -X POST https://azkintun.tudominio.com/api/scrape
```

---

### GET /api/scrape/status

Estado actual del scrape.

**Respuesta 200:**
```json
{
  "scraping": true,
  "startedAt": "2026-08-04T20:15:00Z",
  "lastFinishedAt": "2026-08-04T19:00:00Z",
  "lastTotalNew": 143,
  "lastErrors": [
    "feed https://ejemplo.com/rss: connection timeout"
  ]
}
```

```bash
curl -b cookies.txt https://azkintun.tudominio.com/api/scrape/status
```

Patrón de polling para esperar que termine:

```bash
while true; do
  STATUS=$(curl -s -b cookies.txt https://azkintun.tudominio.com/api/scrape/status)
  SCRAPING=$(echo $STATUS | jq -r .scraping)
  [ "$SCRAPING" = "false" ] && break
  echo "Scraping... $(echo $STATUS | jq -r .startedAt)"
  sleep 2
done
echo "Terminó: $(echo $STATUS | jq .lastTotalNew) artículos nuevos"
```

---

## Import

Los endpoints de import reciben el archivo como `multipart/form-data`. El
nombre del campo puede ser cualquiera (`file`, `upload`, etc.).

### POST /api/import/opml

Importa suscripciones desde un archivo OPML. Compatible con exports de
Inoreader, Feedly, Miniflux y la mayoría de lectores RSS.

**Respuesta 200:**
```json
{
  "foldersCreated": 3,
  "sourcesCreated": 42,
  "sourcesSkipped": 5,
  "errors": [
    "Línea 7: URL inválida 'not-a-url'"
  ]
}
```

```bash
curl -b cookies.txt -X POST https://azkintun.tudominio.com/api/import/opml \
  -F "file=@mis-feeds.opml"
```

---

### POST /api/import/csv

Importa suscripciones desde CSV. El archivo debe tener headers; se detectan
por nombre (no por posición). Columnas reconocidas:

| Columna | Alias aceptados |
|---------|-----------------|
| Nombre | `name`, `title`, `nombre` |
| URL | `url`, `feed url`, `rss_url`, `xmlurl` |
| Carpeta | `folder`, `category`, `carpeta`, `categoría` |

Compatible con el export CSV del propio Azkintun y con exports de Inoreader.

**Respuesta 200:** igual que OPML.

```bash
curl -b cookies.txt -X POST https://azkintun.tudominio.com/api/import/csv \
  -F "file=@feeds.csv"
```

Ejemplo de CSV válido:
```csv
Name,Feed URL,Folder
Krebs on Security,https://krebsonsecurity.com/feed/,Seginfo-Noticias
Dark Reading,https://www.darkreading.com/rss.xml,Seginfo-Noticias
Hacker News,https://hnrss.org/frontpage,
```

---

## Export

Los endpoints de export devuelven el archivo como descarga
(`Content-Disposition: attachment`). Requieren autenticación.

### GET /api/export/opml

Exporta todas las suscripciones en formato OPML 1.0, agrupadas por carpeta.

```bash
curl -b cookies.txt -o mis-feeds.opml \
  https://azkintun.tudominio.com/api/export/opml
```

---

### GET /api/export/csv

Exporta todas las suscripciones como CSV con columnas `Name,Feed URL,Folder`.

```bash
curl -b cookies.txt -o feeds.csv \
  https://azkintun.tudominio.com/api/export/csv
```

---

## Modelos de datos

### Folder

```json
{
  "id": 1,
  "name": "Seginfo-CVE",
  "sourceCount": 12,
  "unreadCount": 342
}
```

### Source

```json
{
  "id": 42,
  "name": "Krebs on Security",
  "rssUrl": "https://krebsonsecurity.com/feed/",
  "folderId": 1,
  "folderName": "Seginfo-Noticias",
  "active": true,
  "custom": false,
  "articleCount": 1240,
  "lastFetch": "2026-08-04T20:00:00Z",
  "lastError": null
}
```

`custom: true` indica que fue agregada manualmente por el usuario.
`custom: false` indica que viene de los feeds sembrados por defecto.

### Article

```json
{
  "id": 9876,
  "title": "Critical CVE-2026-1234 in OpenSSL",
  "summary": "Resumen en texto plano, máx ~280 caracteres.",
  "sourceId": 42,
  "sourceName": "vuldb.com",
  "folderId": 1,
  "folderName": "Seginfo-CVE",
  "severity": "critical",
  "publishedAt": "2026-08-04T18:30:00Z",
  "readTime": "2 MIN READ",
  "url": "https://vuldb.com/?id.12345",
  "imageUrl": "https://vuldb.com/img/logo.png",
  "isRead": false,
  "isStarred": false
}
```

`severity` se infiere del título del artículo por keywords. Valores:
- `critical` — contiene "critical", "0-day", "zero-day", "actively exploited"
- `high` — contiene "high", "exploit", "rce", "ransomware"
- `medium` — contiene "medium", "cve-", "vulnerability", "advisory"
- `low` — todo lo demás

`imageUrl` puede ser `null` si el feed no trae imagen.

`publishedAt` es ISO 8601 UTC, puede ser `null` si el feed no trae fecha.

---

## Construir un frontend desde cero

Flujo mínimo para una UI funcional:

```
1. GET /api/auth/me
   → 200: ya hay sesión, saltar al paso 4
   → 401: mostrar login

2. POST /api/auth/login  {username, password}
   → guardar el token (cookie automática en browser,
     o guardar en memoria para usar como Bearer)

3. Cargar datos iniciales en paralelo:
   GET /api/folders
   GET /api/sources

4. Cargar artículos:
   GET /api/articles?limit=100

5. Al abrir un artículo:
   PATCH /api/articles/:id  {isRead: true}

6. Al hacer click en estrella:
   PATCH /api/articles/:id  {isStarred: true/false}

7. Para actualizar feeds manualmente:
   POST /api/scrape
   → poll GET /api/scrape/status hasta que scraping === false
   → recargar artículos
```

El token tiene TTL de 24h por defecto. Cuando cualquier request devuelva
`401`, redirigir al login.

Las cookies son `SameSite=Strict`, así que si el frontend está en un dominio
diferente al backend, necesitás usar el header `Authorization: Bearer`.
Si frontend y backend están en el mismo dominio (nginx proxy), las cookies
funcionan automáticamente.
