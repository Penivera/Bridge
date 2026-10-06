// Nodes: infrastructure inventory with per-node detail drawer and replication action.
(function () {
  'use strict';

  const FILTERS = [
    { key: 'all', label: 'All' },
    { key: 'healthy', label: 'Healthy' },
    { key: 'suspected', label: 'Suspected' },
    { key: 'dead', label: 'Dead' },
  ];

  function sevClass(severity) {
    const s = String(severity || 'info').toLowerCase();
    if (s === 'critical' || s === 'error') return 'critical';
    if (s === 'warning') return 'warning';
    return 'info';
  }

  function eventRow(e) {
    const sev = sevClass(e.severity);
    return `
      <div class="event-row">
        <div class="event-time">${BridgeUI.fmt.time(e.timestamp)} · ${BridgeUI.fmt.ago(e.timestamp)}</div>
        <div class="event-sev-col"><span class="severity-bar severity-bar--${sev}"></span>${BridgeUI.esc(e.event_type || '–')}</div>
        <div class="event-node">${BridgeUI.esc(e.node_id || '–')}</div>
        <div class="event-message">${BridgeUI.esc(e.message || '')}</div>
      </div>`;
  }

  function detailItem(label, valueHtml, mono) {
    return `<div class="detail-item"><div class="detail-label">${BridgeUI.esc(label)}</div><div class="detail-value${mono ? ' mono' : ''}">${valueHtml}</div></div>`;
  }

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.nodes = {
    title: 'Nodes',
    _root: null,
    _wrap: null,
    _search: '',
    _filter: 'all',
    _state: null,

    render(container, state) {
      this._root = container;
      this._state = state;
      container.innerHTML = `
        <div class="filter-bar" style="margin-bottom:16px">
          <input type="search" class="search-input" id="nodes-search" placeholder="Filter by node ID, mesh IP, or endpoint" value="${BridgeUI.esc(this._search)}">
          ${FILTERS.map(f => `<button class="filter-btn${this._filter === f.key ? ' active' : ''}" data-filter="${f.key}" type="button">${f.label}</button>`).join('')}
        </div>
        <div id="nodes-table-wrap"></div>`;

      const search = container.querySelector('#nodes-search');
      search.addEventListener('input', () => {
        this._search = search.value;
        this._refreshTable();
      });
      container.querySelector('.filter-bar').addEventListener('click', (e) => {
        const btn = e.target.closest('.filter-btn');
        if (!btn) return;
        this._filter = btn.dataset.filter;
        container.querySelectorAll('.filter-btn').forEach(b => b.classList.toggle('active', b === btn));
        this._refreshTable();
      });
      this._wrap = container.querySelector('#nodes-table-wrap');
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

    _filtered(nodes) {
      const q = this._search.trim().toLowerCase();
      return nodes.filter(n => {
        if (this._filter !== 'all' && String(n.health || '').toLowerCase() !== this._filter) return false;
        if (!q) return true;
        return [n.node_id, n.mesh_ip, n.endpoint].some(v => String(v == null ? '' : v).toLowerCase().includes(q));
      });
    },

    _tableHtml(state) {
      if (!Array.isArray(state.nodes)) {
        const body = state.errors && state.errors.nodes
          ? BridgeUI.errorState('nodes unavailable, retrying', state.errors.nodes)
          : BridgeUI.skeletons(1, 220);
        return BridgeUI.panel({ title: 'Nodes', flush: true, body });
      }
      const table = BridgeUI.table({
        columns: [
          { label: 'Node', primary: true, render: n => `<span class="mono">${BridgeUI.esc(n.node_id != null ? n.node_id : '–')}</span>${n.is_leader ? ' ' + BridgeUI.badge('LEADER', 'leader') : ''}` },
          { label: 'Status', render: n => `${BridgeUI.statusDot(n.health)} ${BridgeUI.healthBadge(n.health)}` },
          { label: 'Role', render: n => BridgeUI.badge(n.role || '–', 'neutral') },
          { label: 'Mesh IP', mono: true, render: n => BridgeUI.esc(n.mesh_ip != null ? n.mesh_ip : '–') },
          { label: 'Endpoint', mono: true, render: n => BridgeUI.esc(n.endpoint != null ? n.endpoint : '–') },
          { label: 'Priority', num: true, render: n => BridgeUI.fmt.num(n.priority) },
          { label: 'Uptime', render: n => BridgeUI.fmt.uptime(n.uptime_secs) },
        ],
        rows: this._filtered(state.nodes),
        rowId: n => String(n.node_id != null ? n.node_id : ''),
        empty: state.nodes.length === 0 ? 'No nodes, running standalone' : 'No nodes match the current filters',
      });
      return BridgeUI.panel({ title: 'Nodes', flush: true, body: table });
    },

    async _openDrawer(nodeId) {
      const drawer = BridgeUI.drawer({ title: nodeId, body: BridgeUI.skeletons(2, 100) });
      const body = drawer.querySelector('.drawer-body');
      const res = await BridgeAPI.getNode(nodeId);
      if (!body.isConnected) return;
      if (res.error || !res.data) {
        body.innerHTML = BridgeUI.errorState('Node unavailable', res.error ? res.error.error : 'Not found');
        return;
      }
      const n = res.data;
      const events = (this._state && Array.isArray(this._state.events) ? this._state.events : [])
        .filter(e => e.node_id === nodeId)
        .slice(0, 10);
      body.innerHTML = `
        <div class="detail-grid">
          ${detailItem('Node ID', BridgeUI.esc(n.node_id != null ? n.node_id : '–'), true)}
          ${detailItem('Role', BridgeUI.badge(n.role || '–', 'neutral'))}
          ${detailItem('Health', BridgeUI.healthBadge(n.health))}
          ${detailItem('Priority', BridgeUI.fmt.num(n.priority))}
          ${detailItem('Mesh IP', BridgeUI.esc(n.mesh_ip != null ? n.mesh_ip : '–'), true)}
          ${detailItem('Endpoint', BridgeUI.esc(n.endpoint != null ? n.endpoint : '–'), true)}
          ${detailItem('Uptime', BridgeUI.fmt.uptime(n.uptime_secs))}
        </div>
        <div class="detail-section-title">WireGuard</div>
        ${BridgeUI.kvList([['Mesh IP', n.mesh_ip], ['Endpoint', n.endpoint]])}
        <div class="detail-section-title">Recent Events</div>
        ${events.length ? `<div class="event-list">${events.map(eventRow).join('')}</div>` : BridgeUI.emptyState('No recent events for this node')}
        <div class="detail-section-title">Operations</div>
        <button class="btn btn--outline" id="node-replicate-btn" type="button">Trigger replication</button>`;
      const btn = body.querySelector('#node-replicate-btn');
      btn.addEventListener('click', () => { this._replicate(btn, nodeId); });
    },

    async _replicate(btn, nodeId) {
      const ok = await BridgeUI.confirm({
        title: 'Trigger replication',
        body: `Replicate this node's routes to <span class="mono">${BridgeUI.esc(nodeId)}</span>?`,
        confirmLabel: 'Trigger',
      });
      if (!ok || !btn.isConnected) return;
      BridgeUI.setLoading(btn, true);
      const res = await BridgeAPI.replicate(nodeId);
      if (btn.isConnected) BridgeUI.setLoading(btn, false);
      if (res.error) {
        if (res.status === 503) BridgeUI.toast('Failover duplicator unavailable on this node', 'warning');
        else BridgeUI.toast(res.error.error || 'Replication failed', 'error');
      } else {
        BridgeUI.toast(res.data && res.data.message ? res.data.message : 'Replication triggered', 'success');
      }
    },
  };
})();
