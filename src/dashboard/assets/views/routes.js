// Routes — routing table with per-route detail drawer and traffic stats.
(function () {
  'use strict';

  function routeMetric(state, domain) {
    return state.metrics && state.metrics.routes ? state.metrics.routes[domain] : null;
  }

  function detailItem(label, valueHtml, mono) {
    return `<div class="detail-item"><div class="detail-label">${BridgeUI.esc(label)}</div><div class="detail-value${mono ? ' mono' : ''}">${valueHtml}</div></div>`;
  }

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.routes = {
    title: 'Routes',
    _root: null,
    _wrap: null,
    _search: '',
    _state: null,

    render(container, state) {
      this._root = container;
      this._state = state;
      container.innerHTML = `
        <div class="filter-bar" style="margin-bottom:16px">
          <input type="search" class="search-input" id="routes-search" placeholder="Filter by hostname" value="${BridgeUI.esc(this._search)}">
        </div>
        <div id="routes-table-wrap"></div>`;

      const search = container.querySelector('#routes-search');
      search.addEventListener('input', () => {
        this._search = search.value;
        this._refreshTable();
      });
      this._wrap = container.querySelector('#routes-table-wrap');
      this._wrap.addEventListener('click', (e) => {
        const tr = e.target.closest('tr[data-row-id]');
        if (tr) this._openDrawer(tr.dataset.rowId);
      });
      this._refreshTable();
    },

    update(state) {
      this._state = state;
      if (!this._root || !this._root.isConnected) return;
      this._refreshTable();
    },

    destroy() {
      this._root = null;
      this._wrap = null;
    },

    _refreshTable() {
      if (!this._wrap || !this._wrap.isConnected) return;
      this._wrap.innerHTML = this._tableHtml(this._state || {});
    },

    _tableHtml(state) {
      if (!Array.isArray(state.routes)) {
        const body = state.errors && state.errors.routes
          ? BridgeUI.errorState('routes unavailable — retrying', state.errors.routes)
          : BridgeUI.skeletons(1, 220);
        return BridgeUI.panel({ title: 'Routes', flush: true, body });
      }
      const q = this._search.trim().toLowerCase();
      const rows = state.routes.filter(r =>
        !q || String(r.domain == null ? '' : r.domain).toLowerCase().includes(q));
      const table = BridgeUI.table({
        columns: [
          { label: 'Hostname', primary: true, render: r => `<span class="mono">${BridgeUI.esc(r.domain != null ? r.domain : '—')}</span>` },
          { label: 'Destination', mono: true, render: r => BridgeUI.esc(r.node_id != null ? r.node_id : '—') },
          { label: 'Backend', mono: true, render: r => BridgeUI.esc(r.target_addr != null ? r.target_addr : '—') },
          { label: 'Targets', num: true, render: r => `${BridgeUI.fmt.num(r.targets_count)}${r.has_hash_ring ? ' ' + BridgeUI.badge('hash ring', 'accent') : ''}` },
          { label: 'Requests', render: r => { const m = routeMetric(state, r.domain); return BridgeUI.fmt.num(m && m.requests); } },
          { label: 'Errors', render: r => {
              const m = routeMetric(state, r.domain);
              const text = BridgeUI.fmt.num(m && m.errors);
              return m && m.errors > 0 ? `<span style="color:var(--danger)">${text}</span>` : text;
            } },
          { label: 'P50', render: r => { const m = routeMetric(state, r.domain); return BridgeUI.fmt.ms(m && m.p50_ms); } },
          { label: 'P95', render: r => { const m = routeMetric(state, r.domain); return BridgeUI.fmt.ms(m && m.p95_ms); } },
        ],
        rows,
        rowId: r => String(r.domain != null ? r.domain : ''),
        empty: state.routes.length === 0 ? 'No routes configured' : 'No routes match the current filter',
      });
      return BridgeUI.panel({ title: 'Routes', flush: true, body: table });
    },

    async _openDrawer(domain) {
      const drawer = BridgeUI.drawer({ title: domain, body: BridgeUI.skeletons(2, 100) });
      const body = drawer.querySelector('.drawer-body');
      const res = await BridgeAPI.getRoute(domain);
      if (!body.isConnected) return;
      if (res.error || !res.data) {
        body.innerHTML = BridgeUI.errorState('Route unavailable', res.error ? res.error.error : 'Not found');
        return;
      }
      const r = res.data;
      const m = routeMetric(this._state || {}, domain);
      const targets = Array.isArray(r.targets) ? r.targets : [];
      body.innerHTML = `
        <div class="detail-grid">
          ${detailItem('Hostname', BridgeUI.esc(r.domain != null ? r.domain : '—'), true)}
          ${detailItem('Destination', BridgeUI.esc(r.node_id != null ? r.node_id : '—'))}
          ${detailItem('Backend', BridgeUI.esc(r.target_addr != null ? r.target_addr : '—'), true)}
          ${detailItem('Targets', BridgeUI.fmt.num(r.targets_count != null ? r.targets_count : targets.length))}
          ${detailItem('Hash Ring', BridgeUI.badge(r.has_hash_ring ? 'yes' : 'no', r.has_hash_ring ? 'accent' : 'neutral'))}
        </div>
        <div class="detail-section-title">Targets</div>
        ${BridgeUI.table({
          columns: [
            { label: 'Node', mono: true, render: t => BridgeUI.esc(t.node_id != null ? t.node_id : '—') },
            { label: 'Address', mono: true, render: t => BridgeUI.esc(t.address != null ? t.address : '—') },
          ],
          rows: targets,
          empty: 'No targets',
        })}
        <div class="detail-section-title">Traffic</div>
        ${m
          ? BridgeUI.kvList([
              ['requests', BridgeUI.fmt.num(m.requests)],
              ['errors', BridgeUI.fmt.num(m.errors)],
              ['error rate', BridgeUI.fmt.pct(m.error_rate)],
              ['p50', BridgeUI.fmt.ms(m.p50_ms)],
              ['p95', BridgeUI.fmt.ms(m.p95_ms)],
            ])
          : BridgeUI.emptyState('No traffic recorded')}`;
    },
  };
})();
