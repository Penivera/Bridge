// BRIDGE Fleet Control shell: state, routing, polling, topbar, BridgeUI kit.
//
// View contract (each views/*.js file registers one):
//   window.BridgeViews.<route> = {
//     title: 'Human Name',
//     render(container)            : called once per navigation; may be async
//     update(state)                : called on every poll tick while active
//     onEvent(event, state)        : optional, called for each WS event
//     destroy()                    : optional, called when leaving the view
//   }

'use strict';

const POLL_INTERVAL = (() => {
  const v = parseInt(localStorage.getItem('bridge.pollInterval'), 10);
  return Number.isFinite(v) && v >= 1000 ? v : 5000;
})();

const state = {
  status: null,
  nodes: null,
  routes: null,
  mesh: null,
  metrics: null,
  rates: null,          // derived: { rps, errorRate } from metrics deltas
  events: [],
  config: null,
  handoff: null,
  logs: null,
  errors: {},           // per-slice last error: { status, nodes, routes, metrics, ... }
  connection: { websocket: false, api: true },
};

let currentView = null;
let currentRoute = null;
let pollTimer = null;

/* ------------------------------------------------------------- BridgeUI */

const BridgeUI = {
  esc: (s) => escapeHtml(s),

  fmt: {
    num(n) {
      if (n === null || n === undefined || Number.isNaN(n)) return '–';
      return Number(n).toLocaleString('en-US');
    },
    uptime(secs) {
      if (secs === null || secs === undefined) return '–';
      secs = Math.max(0, Math.floor(secs));
      const d = Math.floor(secs / 86400), h = Math.floor((secs % 86400) / 3600),
            m = Math.floor((secs % 3600) / 60), s = secs % 60;
      if (d > 0) return `${d}d ${h}h`;
      if (h > 0) return `${h}h ${m}m`;
      if (m > 0) return `${m}m ${s}s`;
      return `${s}s`;
    },
    ms(v) {
      if (v === null || v === undefined) return '–';
      return v < 1 ? '<1ms' : `${Math.round(v)}ms`;
    },
    pct(v, digits = 2) {
      if (v === null || v === undefined) return '–';
      return `${(v * 100).toFixed(digits)}%`;
    },
    ago(isoTs) {
      if (!isoTs) return '–';
      const then = new Date(isoTs).getTime();
      if (Number.isNaN(then)) return '–';
      const s = Math.max(0, Math.floor((Date.now() - then) / 1000));
      if (s < 5) return 'just now';
      if (s < 60) return `${s}s ago`;
      if (s < 3600) return `${Math.floor(s / 60)}m ago`;
      if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
      return `${Math.floor(s / 86400)}d ago`;
    },
    time(isoTs) {
      if (!isoTs) return '–';
      const d = new Date(isoTs);
      if (Number.isNaN(d.getTime())) return '–';
      return d.toLocaleTimeString('en-GB', { hour12: false });
    },
  },

  metric({ label, value, sub = '', tone = '', mono = false }) {
    return `
      <div class="metric">
        <div class="metric-label">${escapeHtml(label)}</div>
        <div class="metric-value${tone ? ` metric-value--${tone}` : ''}${mono ? ' mono' : ''}">${escapeHtml(value ?? '–')}</div>
        ${sub ? `<div class="metric-sub">${escapeHtml(sub)}</div>` : ''}
      </div>`;
  },

  metricGrid(metricsHtml) {
    return `<div class="metric-grid">${metricsHtml}</div>`;
  },

  panel({ title, actions = '', body = '', flush = false }) {
    return `
      <section class="panel">
        ${title ? `<div class="panel-header"><span class="panel-title">${escapeHtml(title)}</span>${actions ? `<div class="panel-actions">${actions}</div>` : ''}</div>` : ''}
        <div class="panel-body${flush ? ' panel-body--flush' : ''}">${body}</div>
      </section>`;
  },

  table({ columns, rows, rowId = null, rowClass = null, empty = 'No data' }) {
    if (!rows || rows.length === 0) return BridgeUI.emptyState(empty);
    const thead = columns.map(c =>
      `<th${c.num ? ' class="cell-num"' : ''}>${escapeHtml(c.label)}</th>`).join('');
    const body = rows.map(row => {
      const cls = [
        rowId ? 'row-clickable' : '',
        rowClass ? rowClass(row) : '',
      ].filter(Boolean).join(' ');
      const idAttr = rowId ? ` data-row-id="${escapeHtml(rowId(row))}"` : '';
      const tds = columns.map(c => {
        const raw = c.render ? c.render(row) : row[c.key];
        const cls = [c.primary ? 'cell-primary' : '', c.num ? 'cell-num' : '', c.mono ? 'mono' : '']
          .filter(Boolean).join(' ');
        return `<td${cls ? ` class="${cls}"` : ''}>${raw ?? '–'}</td>`;
      }).join('');
      return `<tr${cls ? ` class="${cls}"` : ''}${idAttr}>${tds}</tr>`;
    }).join('');
    return `<div class="table-wrap"><table class="data-table"><thead><tr>${thead}</tr></thead><tbody>${body}</tbody></table></div>`;
  },

  statusDot(status) {
    const s = String(status || 'unknown').toLowerCase();
    const variant = ['healthy', 'suspected', 'dead'].includes(s) ? s : 'unknown';
    return `<span class="status-dot status-dot--${variant}" title="${escapeHtml(s)}"></span>`;
  },

  badge(text, variant = 'neutral') {
    return `<span class="badge badge--${variant}">${escapeHtml(text)}</span>`;
  },

  healthBadge(status) {
    const s = String(status || 'unknown').toLowerCase();
    const variant = { healthy: 'healthy', suspected: 'warning', dead: 'danger' }[s] || 'neutral';
    return BridgeUI.badge(s, variant);
  },

  emptyState(title, hint = '') {
    return `<div class="empty-state"><div class="empty-title">${escapeHtml(title)}</div>${hint ? `<div>${escapeHtml(hint)}</div>` : ''}</div>`;
  },

  errorState(title, detail = '') {
    return `<div class="error-state"><div class="empty-title">${escapeHtml(title)}</div>${detail ? `<div class="error-detail">${escapeHtml(detail)}</div>` : ''}</div>`;
  },

  skeletons(count = 3, height = 120) {
    return Array.from({ length: count }, () =>
      `<div class="skeleton skeleton--panel" style="height:${height}px"></div>`).join('');
  },

  toast(message, type = 'info', timeout = 4200) {
    const container = document.getElementById('toast-container');
    if (!container) return;
    const el = document.createElement('div');
    el.className = `toast toast--${type}`;
    el.textContent = message;
    container.appendChild(el);
    setTimeout(() => el.remove(), timeout);
  },

  confirm({ title, body, confirmLabel = 'Confirm', danger = false }) {
    return new Promise((resolve) => {
      const root = document.getElementById('modal-root');
      const overlay = document.createElement('div');
      overlay.className = 'modal-overlay';
      overlay.innerHTML = `
        <div class="modal" role="dialog" aria-modal="true">
          <div class="modal-header"><div class="modal-title">${escapeHtml(title)}</div></div>
          <div class="modal-body">${body}</div>
          <div class="modal-actions">
            <button class="btn" data-act="cancel">Cancel</button>
            <button class="btn ${danger ? 'btn--danger' : 'btn--primary'}" data-act="ok">${escapeHtml(confirmLabel)}</button>
          </div>
        </div>`;
      const done = (val) => { overlay.remove(); resolve(val); };
      overlay.addEventListener('click', (e) => {
        if (e.target === overlay) done(false);
        const act = e.target.closest('[data-act]')?.dataset.act;
        if (act === 'ok') done(true);
        if (act === 'cancel') done(false);
      });
      root.appendChild(overlay);
      overlay.querySelector('[data-act="ok"]').focus();
    });
  },

  drawer({ title, body }) {
    BridgeUI.closeDrawer();
    const root = document.getElementById('drawer-root');
    const overlay = document.createElement('div');
    overlay.className = 'drawer-overlay';
    const drawer = document.createElement('aside');
    drawer.className = 'drawer';
    drawer.innerHTML = `
      <div class="drawer-header">
        <div class="drawer-title">${escapeHtml(title)}</div>
        <button class="drawer-close" aria-label="Close">&times;</button>
      </div>
      <div class="drawer-body">${body}</div>`;
    overlay.addEventListener('click', BridgeUI.closeDrawer);
    drawer.querySelector('.drawer-close').addEventListener('click', BridgeUI.closeDrawer);
    root.appendChild(overlay);
    root.appendChild(drawer);
    requestAnimationFrame(() => {
      overlay.classList.add('open');
      drawer.classList.add('open');
    });
    return drawer;
  },

  closeDrawer() {
    const root = document.getElementById('drawer-root');
    if (root) root.innerHTML = '';
  },

  field({ label, name, type = 'text', value = '', options = null, placeholder = '', mono = false, hint = '', disabled = false }) {
    let control;
    if (type === 'select') {
      const opts = (options || []).map(o =>
        `<option value="${escapeHtml(o.value)}"${String(o.value) === String(value) ? ' selected' : ''}>${escapeHtml(o.label)}</option>`).join('');
      control = `<select class="field-select" name="${escapeHtml(name)}"${disabled ? ' disabled' : ''}>${opts}</select>`;
    } else if (type === 'checkbox') {
      return `
        <div class="field" data-field="${escapeHtml(name)}">
          <label class="field-checkbox">
            <input type="checkbox" name="${escapeHtml(name)}"${value ? ' checked' : ''}${disabled ? ' disabled' : ''}>
            <span>${escapeHtml(label)}</span>
          </label>
          ${hint ? `<div class="field-hint">${escapeHtml(hint)}</div>` : ''}
          <div class="field-error"></div>
        </div>`;
    } else if (type === 'textarea') {
      control = `<textarea class="field-textarea${mono ? ' mono' : ''}" name="${escapeHtml(name)}" placeholder="${escapeHtml(placeholder)}"${disabled ? ' disabled' : ''}>${escapeHtml(value)}</textarea>`;
    } else {
      control = `<input class="field-input${mono ? ' mono' : ''}" type="${escapeHtml(type)}" name="${escapeHtml(name)}" value="${escapeHtml(value)}" placeholder="${escapeHtml(placeholder)}"${disabled ? ' disabled' : ''}>`;
    }
    return `
      <div class="field" data-field="${escapeHtml(name)}">
        <label class="field-label">${escapeHtml(label)}</label>
        ${control}
        ${hint ? `<div class="field-hint">${escapeHtml(hint)}</div>` : ''}
        <div class="field-error"></div>
      </div>`;
  },

  setFieldError(fieldEl, message) {
    if (!fieldEl) return;
    fieldEl.classList.toggle('has-error', Boolean(message));
    const err = fieldEl.querySelector('.field-error');
    if (err) err.textContent = message || '';
  },

  setLoading(btn, loading) {
    if (!btn) return;
    if (loading) {
      btn.dataset.prevHtml = btn.innerHTML;
      btn.disabled = true;
      btn.innerHTML = '<span class="spinner"></span>';
    } else {
      btn.disabled = false;
      if (btn.dataset.prevHtml) btn.innerHTML = btn.dataset.prevHtml;
    }
  },

  kvList(pairs) {
    const rows = pairs
      .filter(([, v]) => v !== undefined && v !== null)
      .map(([k, v]) => `<div class="kv-row"><div class="kv-key">${escapeHtml(k)}</div><div class="kv-value">${escapeHtml(v)}</div></div>`)
      .join('');
    return rows ? `<div class="kv-list">${rows}</div>` : BridgeUI.emptyState('No data');
  },
};

window.BridgeUI = BridgeUI;
window.BridgeViews = window.BridgeViews || {};

/* ---------------------------------------------------------------- topbar */

function deriveHealth() {
  if (state.errors.status) return { key: 'down', label: 'Disconnected' };
  if (!state.status) return { key: 'loading', label: 'Loading' };
  const nodes = Array.isArray(state.nodes) ? state.nodes : [];
  const dead = nodes.filter(n => (n.health || '').toLowerCase() === 'dead').length;
  const suspected = nodes.filter(n => (n.health || '').toLowerCase() === 'suspected').length;
  const noLeader = (state.status.node_count || 1) > 1 && !state.status.leader_id;
  if (dead > 0) return { key: 'critical', label: 'Critical' };
  if (suspected > 0 || noLeader) return { key: 'degraded', label: 'Degraded' };
  return { key: 'healthy', label: 'Healthy' };
}

function updateTopbar() {
  const pill = document.getElementById('health-pill');
  const health = deriveHealth();
  if (pill) {
    pill.className = `health-pill health-pill--${health.key}`;
    pill.innerHTML = `<span class="status-dot status-dot--${
      { healthy: 'healthy', degraded: 'warning', critical: 'danger', down: 'danger' }[health.key] || 'unknown'
    }"></span><span class="health-pill-text">${health.label}</span>`;
  }

  const leaderWrap = document.querySelector('.topbar-leader');
  const leaderEl = document.getElementById('leader-display');
  const leader = state.status?.leader_id;
  if (leaderEl) leaderEl.textContent = leader || 'No Leader';
  if (leaderWrap) {
    leaderWrap.classList.toggle('no-leader', !leader);
    leaderWrap.classList.toggle('has-leader', !!leader);
  }

  const identity = document.getElementById('node-identity');
  if (identity) identity.textContent = state.status?.node_id || '–';

  const version = document.getElementById('version-display');
  if (version) version.textContent = `v${state.status?.version || '?.?.?'}`;
  const uptime = document.getElementById('uptime-display');
  if (uptime) uptime.textContent = state.status ? BridgeUI.fmt.uptime(state.status.uptime_secs) : '–';

  const rt = document.getElementById('realtime-indicator');
  const rtText = document.getElementById('realtime-text');
  if (rt && rtText) {
    const connected = state.connection.websocket;
    rt.className = `realtime ${connected ? 'realtime--up' : 'realtime--down'}`;
    rt.innerHTML = `<span class="status-dot status-dot--${connected ? 'healthy' : 'warning'}"></span><span id="realtime-text">${connected ? 'Connected' : 'Reconnecting'}</span>`;
  }
}

/* ----------------------------------------------------------------- poll */

function computeRates(prev, next, dtSecs) {
  if (!prev || !next || dtSecs <= 0) return null;
  let dReq = 0, dErr = 0;
  const routes = next.routes || {};
  for (const [host, m] of Object.entries(routes)) {
    const p = prev.routes?.[host];
    if (!p) continue;
    dReq += Math.max(0, (m.requests || 0) - (p.requests || 0));
    dErr += Math.max(0, (m.errors || 0) - (p.errors || 0));
  }
  return { rps: dReq / dtSecs, errorRate: dReq > 0 ? dErr / dReq : 0 };
}

async function poll() {
  const t0 = Date.now();
  const prevMetrics = state.metrics;

  const [status, nodes, routes, metrics] = await Promise.all([
    BridgeAPI.getStatus(),
    BridgeAPI.getNodes(),
    BridgeAPI.getRoutes(),
    BridgeAPI.getMetrics(),
  ]);

  state.errors.status = status.error ? (status.error.error || 'unavailable') : null;
  state.errors.nodes = nodes.error ? (nodes.error.error || 'unavailable') : null;
  state.errors.routes = routes.error ? (routes.error.error || 'unavailable') : null;
  state.errors.metrics = metrics.error ? (metrics.error.error || 'unavailable') : null;

  if (status.data) state.status = status.data;
  if (nodes.data) state.nodes = nodes.data.nodes || [];
  if (routes.data) state.routes = routes.data.routes || [];
  if (metrics.data) {
    state.metrics = metrics.data;
    state.rates = computeRates(prevMetrics, metrics.data, (Date.now() - t0) / 1000 + (state._lastPollDt || 0)) || state.rates;
  }
  state._lastPollDt = (Date.now() - t0) / 1000;

  state.connection.api = !state.errors.status;
  updateTopbar();
  if (currentView && typeof currentView.update === 'function') {
    try { currentView.update(state); } catch (e) { console.error('view update failed', e); }
  }
}

/* ---------------------------------------------------------------- router */

const ROUTES = {
  overview: 'Overview',
  nodes: 'Nodes',
  mesh: 'Mesh',
  routes: 'Routes',
  requests: 'Requests',
  backends: 'Backends',
  proxy: 'Proxy',
  handoffs: 'Handoffs',
  events: 'Events',
  runtime: 'Runtime',
  cluster: 'Cluster',
  logs: 'Logs',
  health: 'Health',
};

function routeFromHash() {
  const hash = location.hash.replace(/^#\/?/, '').split('?')[0];
  return ROUTES[hash] ? hash : 'overview';
}

async function navigate() {
  const route = routeFromHash();
  if (currentView && typeof currentView.destroy === 'function') {
    try { currentView.destroy(); } catch (_) {}
  }
  BridgeUI.closeDrawer();
  currentRoute = route;

  document.querySelectorAll('.nav-item').forEach(a => {
    a.classList.toggle('active', a.dataset.view === route);
  });
  document.getElementById('topbar-section').textContent = ROUTES[route];
  closeSidebar();

  const content = document.getElementById('content');
  const view = window.BridgeViews[route];
  if (!view) {
    content.innerHTML = BridgeUI.errorState(
      `${ROUTES[route]} view failed to load`,
      'The view module did not register. Check the daemon logs and asset serving.'
    );
    currentView = null;
    return;
  }

  currentView = view;
  content.innerHTML = BridgeUI.skeletons(3);
  try {
    await view.render(content, state);
    if (currentRoute === route && typeof view.update === 'function') view.update(state);
  } catch (e) {
    content.innerHTML = BridgeUI.errorState(`${ROUTES[route]} view crashed while rendering`, e.message);
  }
  content.focus({ preventScroll: true });
}

/* ---------------------------------------------------------------- sidebar */

function openSidebar() {
  document.getElementById('sidebar').classList.add('open');
  document.getElementById('sidebar-overlay').classList.add('open');
}
function closeSidebar() {
  document.getElementById('sidebar').classList.remove('open');
  document.getElementById('sidebar-overlay').classList.remove('open');
}

/* ------------------------------------------------------------------ init */

async function initSession() {
  const auth = await BridgeAPI.getAuth();
  if (auth.status === 401 || (auth.data && auth.data.authenticated === false)) {
    // Middleware would have redirected already; defensive fallback.
    if (location.pathname !== '/login') location.href = '/login';
    return;
  }
  const username = auth.data && auth.data.username ? auth.data.username : null;
  if (username) {
    const meta = document.getElementById('session-meta');
    const userEl = document.getElementById('session-user');
    if (meta) meta.style.display = 'flex';
    if (userEl) userEl.textContent = username;
  }
  const logoutBtn = document.getElementById('logout-btn');
  if (logoutBtn) {
    logoutBtn.addEventListener('click', async () => {
      await BridgeAPI.logout();
      location.href = '/login';
    });
  }
}

function init() {
  window.addEventListener('hashchange', navigate);
  document.getElementById('sidebar-toggle').addEventListener('click', openSidebar);
  document.getElementById('sidebar-close').addEventListener('click', closeSidebar);
  document.getElementById('sidebar-overlay').addEventListener('click', closeSidebar);

  initSession();

  const stream = new BridgeStream();
  stream.onEvent((event) => {
    state.events = [event, ...state.events].slice(0, 500);
    if (currentView && typeof currentView.onEvent === 'function') {
      try { currentView.onEvent(event, state); } catch (_) {}
    }
  });
  document.addEventListener('bridge:ws', (e) => {
    state.connection.websocket = Boolean(e.detail?.connected);
    updateTopbar();
  });
  stream.connect();

  navigate();
  poll();
  pollTimer = setInterval(poll, POLL_INTERVAL);
}

document.addEventListener('DOMContentLoaded', init);
