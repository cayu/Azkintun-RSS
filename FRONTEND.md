# Documentación del Frontend: Azkintun-RSS

**Azkintun-RSS** es una aplicación web tipo *Single Page Application* (SPA) para la lectura y gestión de feeds RSS. Está desarrollada íntegramente en **Vanilla JS** (JavaScript puro), HTML5 y CSS3, sin dependencias de frameworks externos como React, Vue o librerías de estilos como Bootstrap.

---

## 1. Arquitectura y Decisiones Técnicas

*   **Single Page Application (SPA):** Toda la aplicación reside en un solo archivo HTML. Las vistas (Login, Interfaz Principal) se alternan manipulando la propiedad `display` en CSS, y el contenido se inyecta dinámicamente en el DOM.
*   **Gestión de Estado Centralizada:** Utiliza un objeto global `state` en JavaScript para mantener la fuente de la verdad (usuario, carpetas, feeds, artículos, filtros).
*   **Diseño Fluido y Responsivo:** Aprovecha **CSS Grid** (`grid-template-columns: repeat(auto-fill, ... )`) para adaptar las tarjetas de noticias a cualquier tamaño de pantalla. Usa **Variables CSS** para manejar el esquema de colores (modo oscuro nativo).
*   **Delegación de Eventos:** En lugar de asignar eventos a cientos de botones individuales, se utiliza un único *Event Listener* en contenedores superiores (ej. `#sidebar` o `#articles`), utilizando `e.target.closest()` para identificar la acción, mejorando drásticamente el rendimiento.
*   **Seguridad y Saneamiento:** Cuenta con funciones manuales de escape (`esc()` y `escAttr()`) para prevenir ataques XSS al renderizar contenido RSS de terceros mediante `.innerHTML`.
*   **Elementos Nativos HTML5:** Emplea el elemento `<dialog>` para el modal de lectura de artículos, aprovechando su gestión nativa del apilamiento (Top Layer) y el pseudo-elemento `::backdrop` para el efecto de desenfoque.

---

## 2. Código Fuente

### `index.html` (Estructura y Estilos)

Este archivo contiene el esqueleto de la aplicación, dividido en tres bloques lógicos: Login, App (Header + Layout) y el Dialog (Modal de lectura).

```html
<!DOCTYPE html>
<html lang="es">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Azkintun-RSS</title>
  <style>
    :root {
      --bg:       #080a0f;
      --bg2:      #0d1017;
      --bg3:      #141821;
      --border:   #191f2b;
      --border2:  #232c3c;
      --text:     #b8c2cf;
      --text2:    #75849a;
      --text3:    #45566a;
      --blue:     #4c9be8;
      --blue2:    #1f4070;
      --green:    #238636;
      --red:      #e05d5d;
      --yellow:   #e3b341;
      color-scheme: dark;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      height: 100vh; overflow: hidden;
      font-family: ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
      background: var(--bg); color: var(--text); font-size: 14px;
      line-height: 1.5;
    }
    button { font-family: inherit; cursor: pointer; }
    a { color: var(--blue); text-decoration: none; }
    a:hover { text-decoration: underline; }
    ::-webkit-scrollbar { width: 5px; }
    ::-webkit-scrollbar-track { background: transparent; }
    ::-webkit-scrollbar-thumb { background: var(--border2); border-radius: 3px; }

    /* ── LOGIN ── */
    #login { height: 100vh; display: grid; place-items: center; background: var(--bg); }
    .login-card {
      width: min(92vw, 360px);
      background: var(--bg2); border: 1px solid var(--border); border-radius: 14px;
      padding: 32px 28px; box-shadow: 0 8px 40px rgba(0,0,0,.5);
    }
    .login-card h1 { font-size: 22px; color: var(--blue); font-weight: 700; margin-bottom: 24px; }
    .login-card label { display: block; font-size: 12px; color: var(--text2); margin: 14px 0 5px; letter-spacing: .3px; }
    .login-card input {
      width: 100%; padding: 9px 12px; border-radius: 8px;
      border: 1px solid var(--border2); background: var(--bg); color: var(--text);
      font-size: 14px; transition: border-color .15s;
    }
    .login-card input:focus { outline: none; border-color: var(--blue); }
    .login-card button {
      width: 100%; margin-top: 22px; padding: 11px;
      border: none; border-radius: 8px; background: var(--blue); color: #fff;
      font-size: 14px; font-weight: 600; transition: filter .15s;
    }
    .login-card button:hover { filter: brightness(1.1); }
    .err { color: var(--red); font-size: 12px; min-height: 16px; margin-top: 10px; }

    /* ── APP LAYOUT ── */
    #app { display: none; grid-template-rows: 48px 1fr; height: 100vh; }

    /* topbar */
    header {
      display: flex; align-items: center; gap: 10px; padding: 0 16px;
      background: var(--bg2); border-bottom: 1px solid var(--border);
      height: 48px;
    }
    .logo { font-weight: 700; font-size: 15px; color: var(--blue); letter-spacing: .4px; white-space: nowrap; }
    .h-search { flex: 1; max-width: 380px; }
    .h-search input {
      width: 100%; padding: 6px 12px; border-radius: 7px;
      border: 1px solid var(--border2); background: var(--bg3); color: var(--text);
      font-size: 13px;
    }
    .h-search input:focus { outline: none; border-color: var(--blue); }
    .h-spacer { flex: 1; }
    #scrape-status { color: #d29922; font-size: 11px; white-space: nowrap; }
    header .h-btn {
      padding: 5px 11px; border: 1px solid var(--border2); border-radius: 7px;
      background: var(--bg3); color: var(--text2); font-size: 12px;
      transition: border-color .15s, color .15s;
    }
    header .h-btn:hover { border-color: var(--blue); color: var(--text); }
    #user-label { color: var(--text3); font-size: 12px; }

    /* ── BODY SPLIT ── */
    .layout { display: grid; grid-template-columns: 260px 1fr; height: 100%; overflow: hidden; }

    /* ── SIDEBAR ── */
    #sidebar {
      background: var(--bg2); border-right: 1px solid var(--border);
      overflow-y: auto; padding: 10px 6px 20px;
      display: flex; flex-direction: column;
    }
    .sb-section-label {
      font-size: 10px; text-transform: uppercase; letter-spacing: .8px;
      color: var(--text3); padding: 14px 10px 5px; font-weight: 600;
      display: flex; align-items: center; justify-content: space-between;
    }
    .sb-section-label button {
      background: none; border: none; color: var(--text3);
      font-size: 15px; line-height: 1; padding: 0 2px; border-radius: 4px;
    }
    .sb-section-label button:hover { color: var(--blue); background: var(--bg3); }
    .sb-item {
      display: flex; align-items: center; gap: 7px;
      padding: 6px 10px; border-radius: 7px; cursor: pointer;
      font-size: 13px; color: var(--text2); user-select: none;
      transition: background .1s, color .1s;
    }
    .sb-item:hover { background: var(--bg3); color: var(--text); }
    .sb-item.active { background: rgba(76,155,232,.15); color: var(--blue); font-weight: 600; }
    .sb-item .sb-icon { font-size: 13px; width: 18px; text-align: center; opacity: .8; flex-shrink: 0; }
    .sb-item .sb-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .sb-item .sb-badge {
      background: var(--bg3); color: var(--text2);
      border-radius: 10px; padding: 0 7px; font-size: 11px; font-weight: 600;
      flex-shrink: 0;
    }
    .sb-item.active .sb-badge { background: rgba(76,155,232,.2); color: var(--blue); }
    .sb-twisty { font-size: 10px; color: var(--text3); width: 12px; flex-shrink: 0; }
    .sb-folder-actions { display: none; gap: 2px; margin-left: 2px; }
    .sb-item:hover .sb-folder-actions { display: flex; }
    .sb-folder-actions button { background: none; border: none; color: var(--text3); font-size: 13px; padding: 0 3px; border-radius: 3px; }
    .sb-folder-actions button:hover { color: var(--blue); background: var(--bg3); }
    .sb-item.source { padding-left: 26px; font-size: 12px; }
    .sb-item.empty-src { padding-left: 26px; font-size: 12px; color: var(--text3); cursor: default; font-style: italic; }
    .sb-item.empty-src:hover { background: none; }
    .sb-item .err-dot { color: var(--red); font-weight: 900; margin-left: 2px; }
    .sb-divider { height: 1px; background: var(--border); margin: 8px 6px; }
    .sb-actions { display: flex; gap: 5px; padding: 12px 6px 4px; flex-wrap: wrap; }
    .sb-actions button {
      flex: 1; min-width: 60px; padding: 6px 8px; border: 1px solid var(--border2);
      border-radius: 6px; background: var(--bg3); color: var(--text2);
      font-size: 11px; transition: border-color .12s, color .12s;
    }
    .sb-actions button:hover { border-color: var(--blue); color: var(--text); }

    /* ── CONTENT ── */
    #content { display: grid; grid-template-rows: 42px 1fr; overflow: hidden; }
    .toolbar {
      display: flex; align-items: center; gap: 12px; padding: 0 18px;
      border-bottom: 1px solid var(--border); background: var(--bg2); height: 42px;
    }
    .toolbar h2 { font-size: 14px; font-weight: 600; color: var(--text); }
    .toolbar .count { color: var(--text3); font-size: 12px; }
    .toolbar .spacer { flex: 1; }
    .toolbar label { font-size: 12px; color: var(--text2); display: flex; align-items: center; gap: 5px; cursor: pointer; }
    .toolbar button {
      padding: 5px 10px; border: 1px solid var(--border2); border-radius: 6px;
      background: var(--bg3); color: var(--text2); font-size: 12px;
    }
    .toolbar button:hover { border-color: var(--blue); color: var(--text); }

    /* ── CARDS GRID ── */
    #articles { overflow-y: auto; padding: 16px; background: var(--bg); }
    #articles-grid {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
      gap: 12px;
    }
    .article {
      background: var(--bg2); border: 1px solid var(--border); border-radius: 10px;
      overflow: hidden; cursor: pointer; display: flex; flex-direction: column;
      transition: border-color .12s, box-shadow .12s, transform .1s;
    }
    .article:hover {
      border-color: var(--border2); box-shadow: 0 4px 20px rgba(0,0,0,.5);
      transform: translateY(-1px);
    }
    .article.unread { border-top: 2px solid var(--blue); }
    .article.read   { border-top: 2px solid transparent; }
    .art-cover {
      width: 100%; height: 100px; object-fit: cover; object-position: center;
      display: block; background: var(--bg3); flex-shrink: 0;
    }
    .art-cover-ph {
      width: 100%; height: 100px; background: var(--bg3);
      display: flex; align-items: center; justify-content: center;
      font-size: 28px; opacity: .15; flex-shrink: 0;
    }
    .sev-bar { height: 2px; flex-shrink: 0; }
    .sev-bar-critical { background: #e05d5d; }
    .sev-bar-high     { background: #db6d28; }
    .sev-bar-medium   { background: #d29922; }
    .sev-bar-low      { background: transparent; }
    .art-body { padding: 12px 13px 11px; flex: 1; display: flex; flex-direction: column; gap: 6px; }
    .art-head { display: flex; align-items: flex-start; gap: 6px; }
    .sev { width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0; margin-top: 5px; }
    .sev-critical { background: #e05d5d; }
    .sev-high     { background: #db6d28; }
    .sev-medium   { background: #d29922; }
    .sev-low      { background: transparent; }
    .art-title { flex: 1; font-size: 13px; line-height: 1.45; }
    .article.unread .art-title { font-weight: 600; color: #e2eaf3; }
    .article.read   .art-title { color: var(--text2); }
    .star { color: var(--border2); font-size: 14px; cursor: pointer; flex-shrink: 0;
      transition: transform .1s, color .1s; margin-top: 1px; }
    .star.on   { color: var(--yellow); }
    .star:hover { color: var(--yellow); transform: scale(1.2); }
    .art-preview {
      font-size: 12px; color: var(--text3); line-height: 1.5;
      display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
    }
    .art-meta { font-size: 11px; color: var(--text3); margin-top: auto; padding-top: 4px; }
    .empty-state { text-align: center; color: var(--text3); padding: 80px 20px; grid-column: 1/-1; line-height: 2; }

    /* ── DIALOG ARTÍCULO ── */
    #art-dialog {
      border: none; border-radius: 14px; padding: 0;
      background: var(--bg2); color: var(--text);
      width: min(700px, 95vw); max-height: 88vh;
      box-shadow: 0 24px 80px rgba(0,0,0,.85);
      overflow: hidden;
    }
    #art-dialog[open] { display: flex; flex-direction: column; margin: auto; }
    #art-dialog::backdrop { background: rgba(0,0,0,.7); backdrop-filter: blur(3px); }
    .dlg-img { width: 100%; height: 160px; object-fit: cover; display: block; background: var(--bg3); flex-shrink: 0; }
    .dlg-sev-bar { height: 3px; flex-shrink: 0; }
    .dlg-body { padding: 22px 24px 24px; overflow-y: auto; flex: 1; display: flex; flex-direction: column; gap: 10px; }
    .dlg-title { font-size: 18px; font-weight: 700; color: #e2eaf3; line-height: 1.4; }
    .dlg-meta  { font-size: 12px; color: var(--text3); }
    .dlg-summary { font-size: 13px; color: var(--text); line-height: 1.7; }
    .dlg-footer { display: flex; align-items: center; gap: 12px; padding: 12px 24px 16px; border-top: 1px solid var(--border); flex-shrink: 0;}
    .dlg-footer a { font-size: 13px; color: var(--blue); }
    .dlg-star { font-size: 20px; cursor: pointer; color: var(--border2); transition: transform .1s, color .1s; }
    .dlg-star.on { color: var(--yellow); }
    .dlg-star:hover { color: var(--yellow); transform: scale(1.15); }
    .dlg-close { margin-left: auto; padding: 7px 16px; border: 1px solid var(--border2); border-radius: 7px; background: var(--bg3); color: var(--text); font-size: 13px; cursor: pointer; }
    .dlg-close:hover { border-color: var(--blue); }

    /* ── MODAL genérico ── */
    .overlay { position: fixed; inset: 0; background: rgba(0,0,0,.65); backdrop-filter: blur(2px); display: grid; place-items: center; z-index: 100; }
    .modal { width: min(92vw, 440px); background: var(--bg2); border: 1px solid var(--border2); border-radius: 13px; padding: 24px; box-shadow: 0 16px 60px rgba(0,0,0,.7); }
    .modal.wide { width: min(92vw, 640px); }
    .modal h3 { font-size: 16px; font-weight: 700; color: var(--text); margin-bottom: 16px; }
    .modal label { display: block; font-size: 12px; margin: 12px 0 5px; color: var(--text2); }
    .modal input, .modal select { width: 100%; padding: 9px 11px; border-radius: 7px; border: 1px solid var(--border2); background: var(--bg); color: var(--text); font-size: 13px; }
    .modal .actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 20px; }
    .modal .actions button { padding: 9px 16px; border: none; border-radius: 7px; background: var(--blue); color: #fff; font-size: 13px; }
    .modal .actions button.btn-secondary { background: var(--bg3); color: var(--text); border: 1px solid var(--border2); }
    .feed-manage { max-height: 55vh; overflow-y: auto; margin-top: 8px; }
    .feed-row { display: grid; grid-template-columns: 1fr 140px auto auto; align-items: center; gap: 10px; padding: 8px 4px; border-bottom: 1px solid var(--bg3); }
    .feed-row .feed-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
    .feed-row select { padding: 5px 8px; border-radius: 6px; border: 1px solid var(--border2); background: var(--bg); color: var(--text); font-size: 12px; }
    .feed-row .active { font-size: 12px; color: var(--text2); display: flex; align-items: center; gap: 4px; white-space: nowrap; }
    .feed-row .del { padding: 4px 10px; border: 1px solid var(--border2); border-radius: 6px; background: var(--bg3); color: var(--red); font-size: 12px; }
    .modal > .btn-secondary { margin-top: 12px; padding: 9px 16px; border: 1px solid var(--border2); border-radius: 7px; background: var(--bg3); color: var(--text); font-size: 13px; }
  </style>
</head>
<body>

<!-- LOGIN -->
<div id="login">
  <div class="login-card">
    <h1>⚡ Azkintun</h1>
    <label for="u">Usuario</label>
    <input id="u" autocomplete="username" value="admin" />
    <label for="p">Contraseña</label>
    <input id="p" type="password" autocomplete="current-password" />
    <button id="login-btn">Ingresar</button>
    <div id="login-err" class="err"></div>
  </div>
</div>

<!-- APP -->
<div id="app">
  <header>
    <span class="logo">⚡ Azkintun</span>
    <div class="h-search"><input id="search" placeholder="Buscar artículos…" /></div>
    <span id="scrape-status"></span>
    <span class="h-spacer"></span>
    <button class="h-btn" id="refresh-btn" title="Actualizar feeds ahora">⟳ Actualizar</button>
    <span id="user-label"></span>
    <button class="h-btn" id="pw-btn" title="Cambiar contraseña">🔑</button>
    <button class="h-btn" id="logout-btn">Salir</button>
  </header>
  <div class="layout">
    <aside id="sidebar"></aside>
    <section id="content">
      <div class="toolbar">
        <h2 id="view-title"></h2>
        <span class="count" id="view-count"></span>
        <span class="spacer"></span>
        <label><input type="checkbox" id="unread-toggle" /> Solo no leídos</label>
        <button id="markall-btn">Marcar todo leído</button>
      </div>
      <div id="articles"><div id="articles-grid"></div></div>
    </section>
  </div>
</div>

<!-- ARTICLE POPUP -->
<dialog id="art-dialog">
  <div class="dlg-sev-bar" id="dlg-sev-bar"></div>
  <img id="dlg-img" class="dlg-img" loading="lazy" decoding="async" alt="" style="display:none"/>
  <div class="dlg-body">
    <div class="dlg-title" id="dlg-title"></div>
    <div class="dlg-meta"  id="dlg-meta"></div>
    <div class="dlg-summary" id="dlg-summary"></div>
  </div>
  <div class="dlg-footer">
    <span class="dlg-star" id="dlg-star" title="Guardar en favoritos">★</span>
    <a id="dlg-link" href="#" target="_blank" rel="noopener">Abrir artículo ↗</a>
    <button class="dlg-close" id="dlg-close">Cerrar</button>
  </div>
</dialog>

<script src="app.js"></script>
</body>
</html>
```

### `app.js` (Lógica de Negocio y Cliente API)

Este script gestiona toda la interactividad: conexión con el servidor, hidratado de componentes, renderizado de plantillas literales (Template Literals) y captura de eventos.

```javascript
const $ = (id) => document.getElementById(id);
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const escAttr = (s) => esc(s).replace(/'/g, '&#39;');

const state = {
  user: null,
  folders: [],
  sources: [],
  view: { type: 'all' },
  search: '',
  unreadOnly: false,
  articles: [],
  expanded: new Set(),
};

// ── API helper (cookie httpOnly, mismo origen vía proxy nginx) ──
async function api(path, opts = {}) {
  const res = await fetch(path, {
    credentials: 'include',
    headers: opts.body && !(opts.body instanceof FormData) ? { 'Content-Type': 'application/json' } : undefined,
    ...opts,
  });
  if (res.status === 401) { showLogin(); throw new Error('unauthorized'); }
  return res;
}

// ── Auth ──
async function tryRestoreSession() {
  try {
    const res = await fetch('/api/auth/me', { credentials: 'include' });
    if (!res.ok) return showLogin();
    state.user = await res.json();
    await enterApp();
  } catch { showLogin(); }
}
function showLogin() { $('app').style.display = 'none'; $('login').style.display = 'grid'; }
async function doLogin() {
  $('login-err').textContent = '';
  $('login-btn').disabled = true;
  try {
    const res = await fetch('/api/auth/login', {
      method: 'POST', credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username: $('u').value, password: $('p').value }),
    });
    if (res.ok) { state.user = (await res.json()).user; await enterApp(); }
    else { const e = await res.json().catch(() => ({})); $('login-err').textContent = e.error || 'Error de login'; }
  } catch { $('login-err').textContent = 'No se pudo conectar.'; }
  finally { $('login-btn').disabled = false; }
}
async function logout() { await api('/api/auth/logout', { method: 'POST' }).catch(() => {}); showLogin(); }
async function changePassword() {
  const v = await formModal('Cambiar contraseña', [
    { name: 'current', label: 'Contraseña actual', type: 'password' },
    { name: 'next', label: 'Contraseña nueva (mín. 8)', type: 'password' },
  ], 'Cambiar');
  if (!v) return;
  const res = await api('/api/auth/change-password', {
    method: 'POST',
    body: JSON.stringify({ currentPassword: v.current, newPassword: v.next }),
  });
  alert(res.ok ? 'Contraseña actualizada.' : ((await res.json().catch(() => ({}))).error || 'Error'));
}

async function enterApp() {
  $('login').style.display = 'none';
  $('app').style.display = 'grid';
  $('user-label').textContent = state.user.username;
  await Promise.all([loadFolders(), loadSources()]);
  renderSidebar();
  await loadArticles();
}

// ── Data loading ──
async function loadFolders() { state.folders = await (await api('/api/folders')).json(); }
async function loadSources() { state.sources = await (await api('/api/sources')).json(); }

function buildArticleQuery() {
  const p = new URLSearchParams();
  const v = state.view;
  if (v.type === 'folder') p.set('folderId', v.id);
  if (v.type === 'source') p.set('sourceId', v.id);
  if (v.type === 'unread') p.set('unreadOnly', 'true');
  if (v.type === 'starred') p.set('starredOnly', 'true');
  if (state.unreadOnly && v.type !== 'unread') p.set('unreadOnly', 'true');
  if (state.search) p.set('search', state.search);
  p.set('limit', '100');
  return p.toString();
}
async function loadArticles() {
  state.articles = await (await api('/api/articles?' + buildArticleQuery())).json();
  renderArticles();
}
async function refreshAll() { await Promise.all([loadFolders(), loadSources()]); renderSidebar(); await loadArticles(); }

// ── Sidebar ──
function viewTitle() {
  const v = state.view;
  if (v.type === 'all') return 'Todos los artículos';
  if (v.type === 'unread') return 'No leídos';
  if (v.type === 'starred') return 'Favoritos';
  if (v.type === 'folder') { const f = state.folders.find(f => f.id == v.id); return f ? f.name : 'Categoría'; }
  if (v.type === 'source') { const s = state.sources.find(s => s.id == v.id); return s ? s.name : 'Feed'; }
  return '';
}
function renderSidebar() {
  const v = state.view;
  const act = (c) => c ? ' active' : '';
  const byFolder = {}; const uncat = [];
  for (const s of state.sources) { if (s.folderId == null) uncat.push(s); else (byFolder[s.folderId] ||= []).push(s); }

  let h = `
    <div class="sb-item${act(v.type==='all')}" data-view="all">
      <span class="sb-icon">📰</span><span class="sb-name">Todos</span>
    </div>
    <div class="sb-item${act(v.type==='unread')}" data-view="unread">
      <span class="sb-icon">🔵</span><span class="sb-name">No leídos</span>
    </div>
    <div class="sb-item${act(v.type==='starred')}" data-view="starred">
      <span class="sb-icon">⭐</span><span class="sb-name">Favoritos</span>
    </div>
    <div class="sb-divider"></div>
    <div class="sb-section-label">Categorías <button data-action="add-folder" title="Nueva categoría">+</button></div>`;

  for (const f of state.folders) {
    const open = state.expanded.has(f.id);
    h += `<div class="folder">
      <div class="sb-item folder-head${act(v.type==='folder'&&v.id==f.id)}">
        <span class="sb-twisty" data-toggle="${f.id}">${open?'▾':'▸'}</span>
        <span class="sb-name" data-view="folder" data-id="${f.id}">${esc(f.name)}</span>${f.unreadCount ? `<span class="sb-badge">${f.unreadCount > 999 ? '999+' : f.unreadCount}</span>` : ''}
        <span class="sb-folder-actions">
          <button data-action="rename-folder" data-id="${f.id}" title="Renombrar">✎</button>
          <button data-action="delete-folder" data-id="${f.id}" title="Eliminar">×</button>
        </span>
      </div>`;
    if (open) {
      const srcs = byFolder[f.id] || [];
      for (const s of srcs)
        h += `<div class="sb-item source${act(v.type==='source'&&v.id==s.id)}" data-view="source" data-id="${s.id}"><span class="sb-name">${esc(s.name)}</span>${s.lastError?`<span class="err-dot" title="${escAttr(s.lastError)}">!</span>`:''}</div>`;
      if (!srcs.length) h += `<div class="sb-item empty-src">sin feeds</div>`;
    }
    h += `</div>`;
  }

  if (uncat.length) {
    const open = state.expanded.has('unc');
    h += `<div class="folder">
      <div class="sb-item folder-head">
        <span class="sb-twisty" data-toggle="unc">${open?'▾':'▸'}</span>
        <span class="sb-name">Sin categoría</span>
      </div>`;
    if (open) for (const s of uncat)
      h += `<div class="sb-item source${act(v.type==='source'&&v.id==s.id)}" data-view="source" data-id="${s.id}"><span class="sb-name">${esc(s.name)}</span></div>`;
    h += `</div>`;
  }

  h += `<div class="sb-divider"></div>
  <div class="sb-actions">
    <button data-action="add-feed">+ Feed</button>
    <button data-action="manage-feeds">Gestionar</button>
    <button data-action="import">Importar</button>
    <button data-action="export">Exportar</button>
  </div>`;
  $('sidebar').innerHTML = h;
}

// ── Articles ──
function relTime(iso) {
  if (!iso) return '';
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (s < 60) return 'ahora';
  if (s < 3600) return Math.floor(s/60) + 'm';
  if (s < 86400) return Math.floor(s/3600) + 'h';
  if (s < 604800) return Math.floor(s/86400) + 'd';
  return new Date(iso).toLocaleDateString();
}
function renderArticles() {
  $('view-title').textContent = viewTitle();
  $('view-count').textContent = state.articles.length ? `${state.articles.length} artículo(s)` : '';
  const grid = $('articles-grid');
  if (!state.articles.length) {
    grid.innerHTML = `<div class="empty-state">No hay artículos que mostrar.<br>Probá actualizar los feeds o cambiar el filtro.</div>`;
    return;
  }

  const sevPh = { critical:'🔴', high:'🟠', medium:'🟡', low:'📰' };

  grid.innerHTML = state.articles.map(a => {
    const sev = esc(a.severity);
    const cover = a.imageUrl
      ? `<img class="art-cover" src="${escAttr(a.imageUrl)}" loading="lazy" decoding="async" alt="" />`
      : `<div class="art-cover-ph">${sevPh[a.severity] || '📰'}</div>`;

    return `<div class="article ${a.isRead?'read':'unread'}" data-article="${a.id}">
      ${cover}
      <div class="sev-bar sev-bar-${sev}"></div>
      <div class="art-body">
        <div class="art-head">
          <span class="sev sev-${sev}"></span>
          <span class="art-title">${esc(a.title)}</span>
          <span class="star ${a.isStarred?'on':''}" data-star="${a.id}" title="Guardar en favoritos">★</span>
        </div>
        ${a.summary ? `<div class="art-preview">${esc(a.summary)}</div>` : ''}
        <div class="art-meta">${esc(a.sourceName)} ·${relTime(a.publishedAt)}</div>
      </div>
    </div>`;
  }).join('');
}

// ── Article dialog ──
const dlg       = $('art-dialog');
const dlgSevBar = $('dlg-sev-bar');
const dlgImg    = $('dlg-img');
const dlgTitle  = $('dlg-title');
const dlgMeta   = $('dlg-meta');
const dlgSum    = $('dlg-summary');
const dlgLink   = $('dlg-link');
const dlgStar   = $('dlg-star');

function openArticleDialog(a) {
  dlgSevBar.className = `dlg-sev-bar sev-bar-${a.severity}`;
  if (a.imageUrl) {
    dlgImg.src = a.imageUrl;
    dlgImg.style.display = 'block';
  } else {
    dlgImg.style.display = 'none';
    dlgImg.src = '';
  }
  dlgTitle.textContent  = a.title;
  dlgMeta.textContent   = [a.sourceName, a.folderName, relTime(a.publishedAt), a.readTime]
                            .filter(Boolean).join(' · ');
  dlgSum.textContent    = a.summary || '(sin resumen)';
  dlgLink.href          = a.url;
  dlgStar.className     = `dlg-star${a.isStarred ? ' on' : ''}`;
  dlgStar.dataset.id    = a.id;
  dlg.showModal();
}

dlg.addEventListener('click', e => { if (e.target === dlg) dlg.close(); });
$('dlg-close').addEventListener('click', () => dlg.close());

dlgStar.addEventListener('click', async e => {
  e.stopPropagation();
  const id = Number(dlgStar.dataset.id);
  const a  = state.articles.find(x => x.id === id);
  if (!a) return;
  const res = await api(`/api/articles/${id}`, { method:'PATCH', body: JSON.stringify({ isStarred: !a.isStarred }) });
  a.isStarred = (await res.json()).isStarred;
  dlgStar.className = `dlg-star${a.isStarred ? ' on' : ''}`;
  renderArticles();
});

dlgImg.addEventListener('error', () => { dlgImg.style.display = 'none'; });

async function openArticle(id) {
  const a = state.articles.find(x => x.id == id);
  if (!a) return;
  if (!a.isRead) {
    a.isRead = true;
    renderArticles();
    api(`/api/articles/${id}`, { method: 'PATCH', body: JSON.stringify({ isRead: true }) })
      .then(() => loadFolders()).then(renderSidebar).catch(() => {});
  }
  openArticleDialog(a);
}
async function toggleStar(id) {
  const a = state.articles.find(x => x.id == id);
  if (!a) return;
  const res = await api('/api/articles/' + id, { method: 'PATCH', body: JSON.stringify({ isStarred: !a.isStarred }) });
  a.isStarred = (await res.json()).isStarred;
  renderArticles();
}
async function markAllRead() {
  const p = new URLSearchParams();
  if (state.view.type === 'folder') p.set('folderId', state.view.id);
  if (state.view.type === 'source') p.set('sourceId', state.view.id);
  await api('/api/articles/mark-all-read?' + p.toString(), { method: 'POST' });
  await refreshAll();
}

// ── Folder / source management ──
async function addFolder() {
  const name = prompt('Nombre de la nueva categoría:');
  if (!name || !name.trim()) return;
  const res = await api('/api/folders', { method: 'POST', body: JSON.stringify({ name: name.trim() }) });
  if (res.ok) { await loadFolders(); renderSidebar(); }
}
async function renameFolder(id) {
  const f = state.folders.find(f => f.id == id);
  const name = prompt('Nuevo nombre:', f ? f.name : '');
  if (!name || !name.trim()) return;
  const res = await api('/api/folders/' + id, { method: 'PATCH', body: JSON.stringify({ name: name.trim() }) });
  if (res.ok) { await loadFolders(); renderSidebar(); }
}
async function deleteFolder(id) {
  if (!confirm('¿Eliminar esta categoría? Sus feeds quedarán sin categoría.')) return;
  await api('/api/folders/' + id, { method: 'DELETE' });
  if (state.view.type === 'folder' && state.view.id == id) state.view = { type: 'all' };
  await refreshAll();
}
async function addFeed() {
  const opts = [{ value: '', label: 'Sin categoría' }, ...state.folders.map(f => ({ value: String(f.id), label: f.name }))];
  const v = await formModal('Agregar feed', [
    { name: 'name', label: 'Nombre', placeholder: 'Ej: Krebs on Security' },
    { name: 'rssUrl', label: 'URL del RSS', placeholder: 'https://…/feed' },
    { name: 'folderId', label: 'Categoría', type: 'select', options: opts },
  ], 'Agregar');
  if (!v || !v.name.trim() || !v.rssUrl.trim()) return;
  const body = { name: v.name.trim(), rssUrl: v.rssUrl.trim() };
  if (v.folderId) body.folderId = Number(v.folderId);
  const res = await api('/api/sources', { method: 'POST', body: JSON.stringify(body) });
  if (res.ok) await refreshAll();
}
async function manageFeeds() {
  await loadSources();
  const overlay = document.createElement('div'); overlay.className = 'overlay';
  const folderOpts = (s) => ['<option value="">Sin categoría</option>',
    ...state.folders.map(f => `<option value="${f.id}" ${f.id==s.folderId?'selected':''}>${esc(f.name)}</option>`)].join('');
  const rows = state.sources.map(s => `<div class="feed-row">
      <span class="feed-name" title="${escAttr(s.rssUrl)}">${esc(s.name)}</span>
      <select class="move" data-id="${s.id}">${folderOpts(s)}</select>
      <label class="active"><input type="checkbox" class="act" data-id="${s.id}" ${s.active?'checked':''}/> activo</label>
      <button class="del" data-id="${s.id}">Eliminar</button>
    </div>`).join('');
  overlay.innerHTML = `<div class="modal wide"><h3>Gestionar feeds (${state.sources.length})</h3>
    <div class="feed-manage">${rows || '<p style="color:#6e7681">No hay feeds.</p>'}</div>
    <button class="btn-secondary close">Cerrar</button></div>`;
  document.body.appendChild(overlay);
  const close = () => { document.body.removeChild(overlay); refreshAll(); };
  overlay.querySelector('.close').onclick = close;
  overlay.onclick = (e) => { if (e.target === overlay) close(); };
  overlay.querySelectorAll('select.move').forEach(sel => sel.onchange = () =>
    api('/api/sources/' + sel.dataset.id, { method: 'PATCH', body: JSON.stringify({ folderId: sel.value ? Number(sel.value) : 0 }) }));
  overlay.querySelectorAll('input.act').forEach(cb => cb.onchange = () =>
    api('/api/sources/' + cb.dataset.id, { method: 'PATCH', body: JSON.stringify({ active: cb.checked }) }));
  overlay.querySelectorAll('button.del').forEach(b => b.onclick = async () => {
    if (!confirm('¿Eliminar este feed y sus artículos?')) return;
    await api('/api/sources/' + b.dataset.id, { method: 'DELETE' });
    b.closest('.feed-row').remove();
  });
}
async function exportFeeds() {
  const v = await formModal('Exportar suscripciones', [
    { name: 'fmt', label: 'Formato', type: 'select', options: [
      { value: 'opml', label: 'OPML — para Inoreader, Feedly, Miniflux...' },
      { value: 'csv',  label: 'CSV — para hojas de cálculo o reimportar' },
    ]},
  ], 'Descargar');
  if (!v) return;
  const a = document.createElement('a');
  a.href = `/api/export/${v.fmt}`;
  a.download = `azkintun-subscriptions.${v.fmt}`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
}
function importFeeds() {
  const input = document.createElement('input');
  input.type = 'file'; input.accept = '.opml,.xml,.csv';
  input.onchange = async () => {
    const file = input.files[0]; if (!file) return;
    const endpoint = file.name.toLowerCase().endsWith('.csv') ? '/api/import/csv' : '/api/import/opml';
    const fd = new FormData(); fd.append('file', file);
    const res = await api(endpoint, { method: 'POST', body: fd });
    const d = await res.json().catch(() => ({}));
    alert(res.ok
      ? `Importado: ${d.sourcesCreated} feeds nuevos,${d.foldersCreated} categorías.`
      : (d.error || 'Error al importar'));
    await refreshAll();
  };
  input.click();
}

// ── Scrape ──
let scrapePoll = null;
async function scrapeNow() {
  await api('/api/scrape', { method: 'POST' });
  $('scrape-status').textContent = 'Actualizando feeds…';
  if (scrapePoll) clearInterval(scrapePoll);
  scrapePoll = setInterval(async () => {
    const s = await (await api('/api/scrape/status')).json();
    if (!s.scraping) {
      clearInterval(scrapePoll); scrapePoll = null;
      $('scrape-status').textContent = s.lastTotalNew != null ? `✓ ${s.lastTotalNew} nuevos` : '';
      setTimeout(() => { $('scrape-status').textContent = ''; }, 4000);
      await refreshAll();
    }
  }, 2000);
}

// ── Generic form modal ──
function formModal(title, fields, submitLabel = 'Guardar') {
  return new Promise((resolve) => {
    const overlay = document.createElement('div'); overlay.className = 'overlay';
    const inputsHtml = fields.map(f => {
      if (f.type === 'select')
        return `<label>${esc(f.label)}</label><select data-name="${f.name}">${f.options.map(o => `<option value="${escAttr(o.value)}">${esc(o.label)}</option>`).join('')}</select>`;
      return `<label>${esc(f.label)}</label><input data-name="${f.name}" type="${f.type||'text'}" placeholder="${escAttr(f.placeholder\vert{}\vert{}'')}" value="${escAttr(f.value||'')}" />`;
    }).join('');
    overlay.innerHTML = `<div class="modal"><h3>${esc(title)}</h3>${inputsHtml}
      <div class="actions"><button class="btn-secondary cancel">Cancelar</button><button class="ok">${esc(submitLabel)}</button></div></div>`;
    document.body.appendChild(overlay);
    const done = (val) => { document.body.removeChild(overlay); resolve(val); };
    overlay.querySelector('.cancel').onclick = () => done(null);
    overlay.onclick = (e) => { if (e.target === overlay) done(null); };
    overlay.querySelector('.ok').onclick = () => {
      const val = {};
      overlay.querySelectorAll('[data-name]').forEach(el => val[el.dataset.name] = el.value);
      done(val);
    };
    const first = overlay.querySelector('input,select'); if (first) first.focus();
  });
}

// ── Event wiring ──
function selectView(view) { state.view = view; renderSidebar(); loadArticles(); }

$('sidebar').addEventListener('click', (e) => {
  const t = e.target.closest('[data-toggle]');
  if (t) { const k = t.dataset.toggle; const id = k === 'unc' ? 'unc' : Number(k); state.expanded.has(id) ? state.expanded.delete(id) : state.expanded.add(id); renderSidebar(); return; }
  const a = e.target.closest('[data-action]');
  if (a) {
    const id = a.dataset.id;
    ({ 'add-folder': addFolder, 'rename-folder': () => renameFolder(id), 'delete-folder': () => deleteFolder(id),
       'add-feed': addFeed, 'manage-feeds': manageFeeds, 'import': importFeeds, 'export': exportFeeds }[a.dataset.action] || (() => {}))();
    return;
  }
  const view = e.target.closest('[data-view]');
  if (view) { const type = view.dataset.view; selectView(type === 'folder' || type === 'source' ? { type, id: Number(view.dataset.id) } : { type }); }
});

$('articles-grid').addEventListener('error', (e) => {
  if (e.target.tagName === 'IMG') e.target.style.display = 'none';
}, true);

$('articles').addEventListener('click', (e) => {
  const star = e.target.closest('[data-star]');
  if (star) { toggleStar(star.dataset.star); return; }
  const art = e.target.closest('[data-article]');
  if (art) openArticle(art.dataset.article);
});

let searchTimer = null;
$('search').addEventListener('input', (e) => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { state.search = e.target.value.trim(); loadArticles(); }, 300);
});
$('unread-toggle').addEventListener('change', (e) => { state.unreadOnly = e.target.checked; loadArticles(); });
$('markall-btn').addEventListener('click', markAllRead);
$('refresh-btn').addEventListener('click', scrapeNow);
$('logout-btn').addEventListener('click', logout);
$('pw-btn').addEventListener('click', changePassword);
$('login-btn').addEventListener('click', doLogin);
$('p').addEventListener('keydown', (e) => { if (e.key === 'Enter') doLogin(); });

// ── Start ──
tryRestoreSession();
```

---

## 3. Referencia de la API (Backend esperado)

El frontend está diseñado para consumir una API RESTful alojada en `/api/`. Todas las llamadas (excepto la exportación) envían el encabezado `credentials: 'include'` para la autenticación por cookies HttpOnly.

| Endpoint | Método | Descripción de la función esperada |
| :--- | :--- | :--- |
| `/api/auth/me` | GET | Devuelve los datos del usuario actual para validar sesión activa. |
| `/api/auth/login` | POST | Valida credenciales (`username`, `password`) e inicia sesión. |
| `/api/auth/logout` | POST | Destruye la sesión activa. |
| `/api/auth/change-password` | POST | Permite modificar la clave del usuario autenticado. |
| `/api/folders` | GET / POST | Lista todas las categorías / Crea una nueva categoría. |
| `/api/folders/:id` | PATCH / DELETE | Renombra o elimina una categoría específica. |
| `/api/sources` | GET / POST | Lista todas las suscripciones (feeds) / Añade un feed nuevo. |
| `/api/sources/:id` | PATCH / DELETE | Modifica (activa/desactiva/mueve de carpeta) o elimina un feed. |
| `/api/articles` | GET | Devuelve la lista de noticias, aceptando filtros por query param (`folderId`, `sourceId`, `unreadOnly`, `starredOnly`, `search`, `limit`). |
| `/api/articles/:id` | PATCH | Modifica el estado de una noticia (`isRead`, `isStarred`). |
| `/api/articles/mark-all-read` | POST | Marca como leídas las noticias (soporta query parameters para limitar el alcance). |
| `/api/scrape` | POST | Inicia un trabajo en segundo plano para actualizar (descargar) los feeds. |
| `/api/scrape/status` | GET | Retorna el estado del actualizador en segundo plano (polling). |
| `/api/export/:fmt` | GET | Descarga directamente las suscripciones en formato `opml` o `csv`. |
| `/api/import/opml` (o `csv`) | POST | Recibe un `FormData` (archivo) para importar feeds y carpetas. |
