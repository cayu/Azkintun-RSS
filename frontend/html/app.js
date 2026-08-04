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

  let h = `<div class="section">
    <div class="item${act(v.type==='all')}" data-view="all">📰 Todos</div>
    <div class="item${act(v.type==='unread')}" data-view="unread">🔵 No leídos</div>
    <div class="item${act(v.type==='starred')}" data-view="starred">⭐ Favoritos</div>
  </div>
  <div class="section-title">Categorías <button data-action="add-folder" title="Nueva categoría">+</button></div>`;

  for (const f of state.folders) {
    const open = state.expanded.has(f.id);
    h += `<div class="folder">
      <div class="item folder-head${act(v.type==='folder'&&v.id==f.id)}">
        <span class="twisty" data-toggle="${f.id}">${open?'▾':'▸'}</span>
        <span class="folder-name" data-view="folder" data-id="${f.id}">${esc(f.name)}</span>
        ${f.unreadCount ? `<span class="badge">${f.unreadCount}</span>` : ''}
        <span class="folder-actions">
          <button data-action="rename-folder" data-id="${f.id}" title="Renombrar">✎</button>
          <button data-action="delete-folder" data-id="${f.id}" title="Eliminar">×</button>
        </span>
      </div>`;
    if (open) {
      const srcs = byFolder[f.id] || [];
      for (const s of srcs)
        h += `<div class="item source${act(v.type==='source'&&v.id==s.id)}" data-view="source" data-id="${s.id}">${esc(s.name)}${s.lastError?` <span class="err" title="${escAttr(s.lastError)}">!</span>`:''}</div>`;
      if (!srcs.length) h += `<div class="item empty">sin feeds</div>`;
    }
    h += `</div>`;
  }

  if (uncat.length) {
    const open = state.expanded.has('unc');
    h += `<div class="folder">
      <div class="item folder-head"><span class="twisty" data-toggle="unc">${open?'▾':'▸'}</span>
      <span class="folder-name">Sin categoría</span></div>`;
    if (open) for (const s of uncat)
      h += `<div class="item source${act(v.type==='source'&&v.id==s.id)}" data-view="source" data-id="${s.id}">${esc(s.name)}</div>`;
    h += `</div>`;
  }

  h += `<div class="sidebar-actions">
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

  const sevIcon = { critical:'🔴', high:'🟠', medium:'🟡', low:'⚪' };

  grid.innerHTML = state.articles.map(a => {
    const sev = esc(a.severity);
    const thumb = a.imageUrl
      ? `<img src="${escAttr(a.imageUrl)}" width="64" height="64" loading="lazy" decoding="async" alt=""
           style="width:64px;height:64px;object-fit:cover;object-position:center;display:block;border-radius:7px;flex-shrink:0"/>`
      : `<div class="art-thumb-ph">${sevIcon[a.severity] || '📰'}</div>`;

    return `<div class="article ${a.isRead?'read':'unread'}" data-article="${a.id}">
      <div class="sev-bar sev-bar-${sev}"></div>
      <div class="art-card-body">
        <div class="art-thumb-box">${thumb}</div>
        <div class="art-text">
          <div class="art-head">
            <span class="sev sev-${sev}"></span>
            <span class="art-title">${esc(a.title)}</span>
            <span class="star ${a.isStarred?'on':''}" data-star="${a.id}" title="Guardar en favoritos">★</span>
          </div>
          ${a.summary ? `<div class="art-preview">${esc(a.summary)}</div>` : ''}
          <div class="art-meta">${esc(a.sourceName)} · ${relTime(a.publishedAt)}</div>
        </div>
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
  // Barra de severidad
  dlgSevBar.className = `dlg-sev-bar sev-bar-${a.severity}`;

  // Imagen (solo si existe y carga bien)
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

// Cerrar al hacer click en el backdrop (área fuera del dialog)
dlg.addEventListener('click', e => { if (e.target === dlg) dlg.close(); });
$('dlg-close').addEventListener('click', () => dlg.close());

// Favorito desde el dialog
dlgStar.addEventListener('click', async e => {
  e.stopPropagation();
  const id = Number(dlgStar.dataset.id);
  const a  = state.articles.find(x => x.id === id);
  if (!a) return;
  const res = await api(`/api/articles/${id}`, { method:'PATCH', body: JSON.stringify({ isStarred: !a.isStarred }) });
  a.isStarred = (await res.json()).isStarred;
  dlgStar.className = `dlg-star${a.isStarred ? ' on' : ''}`;
  renderArticles(); // actualiza la estrella en la card también
});

// Ocultar imagen del dialog si falla
dlgImg.addEventListener('error', () => { dlgImg.style.display = 'none'; });

async function openArticle(id) {
  const a = state.articles.find(x => x.id == id);
  if (!a) return;
  // Marcar leído si aún no lo está
  if (!a.isRead) {
    a.isRead = true;
    renderArticles(); // actualiza la card inmediatamente
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
  else alert((await res.json().catch(() => ({}))).error || 'Error');
}
async function renameFolder(id) {
  const f = state.folders.find(f => f.id == id);
  const name = prompt('Nuevo nombre:', f ? f.name : '');
  if (!name || !name.trim()) return;
  const res = await api('/api/folders/' + id, { method: 'PATCH', body: JSON.stringify({ name: name.trim() }) });
  if (res.ok) { await loadFolders(); renderSidebar(); }
  else alert((await res.json().catch(() => ({}))).error || 'Error');
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
  else alert((await res.json().catch(() => ({}))).error || 'Error');
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
  // Descarga directa usando un <a> temporal; el servidor manda el
  // header Content-Disposition: attachment, así que el browser lo guarda.
  const a = document.createElement('a');
  a.href = `/api/export/${v.fmt}`;
  a.download = `azkintun-subscriptions.${v.fmt}`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
}
function importFeeds() {
  input.type = 'file'; input.accept = '.opml,.xml,.csv';
  input.onchange = async () => {
    const file = input.files[0]; if (!file) return;
    const endpoint = file.name.toLowerCase().endsWith('.csv') ? '/api/import/csv' : '/api/import/opml';
    const fd = new FormData(); fd.append('file', file);
    const res = await api(endpoint, { method: 'POST', body: fd });
    const d = await res.json().catch(() => ({}));
    alert(res.ok
      ? `Importado: ${d.sourcesCreated} feeds nuevos, ${d.foldersCreated} categorías, ${d.sourcesSkipped} ya existían.` + (d.errors && d.errors.length ? '\n\nAvisos:\n' + d.errors.join('\n') : '')
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
      return `<label>${esc(f.label)}</label><input data-name="${f.name}" type="${f.type||'text'}" placeholder="${escAttr(f.placeholder||'')}" value="${escAttr(f.value||'')}" />`;
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

// Ocultar imágenes que fallan al cargar (el evento 'error' no burbujea,
// por eso se escucha en fase de captura). Reemplaza al onerror inline,
// que el CSP bloquea.
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
