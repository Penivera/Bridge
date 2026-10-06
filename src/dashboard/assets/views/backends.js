// Backends: derived view: one row per route target, with the owning node's health.
(function () {
  'use strict';

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.backends = {
    title: 'Backends',
    _root: null,

    render(container, state) {
      this._root = container;
      this.update(state);
    },

    update(state) {
      if (!this._root || !this._root.isConnected) return;

      if (!Array.isArray(state.routes)) {
        this._root.innerHTML = state.errors.routes
          ? BridgeUI.errorState('routes unavailable, retrying', state.errors.routes)
          : BridgeUI.skeletons(1, 220);
        return;
      }

      const healthByNode = new Map();
      if (Array.isArray(state.nodes)) {
        state.nodes.forEach(n => healthByNode.set(n.node_id, n.health));
      }

      const rows = [];
      state.routes.forEach(r => {
        (Array.isArray(r.targets) ? r.targets : []).forEach(t => {
          rows.push({
            address: t.address,
            domain: r.domain,
            node_id: t.node_id,
            // API-provided liveness wins; fall back to the nodes view map.
            health: t.health || healthByNode.get(t.node_id) || 'unknown',
            ring: Boolean(r.has_hash_ring),
          });
        });
      });

      const body = BridgeUI.table({
        columns: [
          { label: 'Backend', primary: true, render: b => `<span class="mono">${BridgeUI.esc(b.address != null ? b.address : '–')}</span>` },
          { label: 'Route', mono: true, render: b => BridgeUI.esc(b.domain != null ? b.domain : '–') },
          { label: 'Node', mono: true, render: b => BridgeUI.esc(b.node_id != null ? b.node_id : '–') },
          { label: 'Node Health', render: b => `${BridgeUI.statusDot(b.health)} ${BridgeUI.healthBadge(b.health)}` },
          { label: 'Hash Ring', render: b => (b.ring ? BridgeUI.badge('ring', 'accent') : '–') },
        ],
        rows,
        empty: 'No backends, no routes configured',
      });
      this._root.innerHTML = BridgeUI.panel({ title: 'Backends', flush: true, body });
    },

    destroy() {
      this._root = null;
    },
  };
})();
