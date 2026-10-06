// Mesh — WireGuard/gossip topology visualization and peer list.
(function () {
  'use strict';

  function layout(nodes) {
    const pos = new Map();
    const n = nodes.length;
    nodes.forEach((node, i) => {
      let x, y;
      if (n === 1) {
        x = 400; y = 210;
      } else if (n === 2) {
        x = i === 0 ? 250 : 550; y = 210;
      } else {
        const angle = -Math.PI / 2 + (i * 2 * Math.PI) / n;
        x = 400 + 150 * Math.cos(angle);
        y = 210 + 150 * Math.sin(angle);
      }
      pos.set(node.node_id, { x: Math.round(x * 10) / 10, y: Math.round(y * 10) / 10 });
    });
    return pos;
  }

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.mesh = {
    title: 'Mesh',
    _root: null,
    _mesh: null,
    _error: null,
    _fetching: false,

    async render(container, state) {
      this._root = container;
      container.innerHTML = `
        <div id="mesh-banner-slot"></div>
        <div id="mesh-content">${BridgeUI.skeletons(2, 200)}</div>`;
      this._renderBanner(state);
      if (this._mesh || this._error) this._renderBody();
      await this._fetchMesh();
    },

    update(state) {
      if (!this._root || !this._root.isConnected) return;
      this._renderBanner(state);
      this._fetchMesh();
    },

    destroy() {
      this._root = null;
    },

    _renderBanner(state) {
      const slot = this._root.querySelector('#mesh-banner-slot');
      if (!slot) return;
      const conv = state.status && state.status.convergence_state;
      if (!conv) {
        slot.innerHTML = '';
        return;
      }
      let cls, text;
      if (conv === 'converged') {
        cls = 'mesh-banner--converged'; text = 'Mesh converged — gossip is healthy';
      } else if (conv === 'standalone') {
        cls = 'mesh-banner--standalone'; text = 'Standalone mode — no cluster peers';
      } else {
        cls = 'mesh-banner--diverged'; text = 'Mesh diverged — check node health';
      }
      slot.innerHTML = `<div class="mesh-banner ${cls}">${text}</div>`;
    },

    async _fetchMesh() {
      if (this._fetching) return;
      this._fetching = true;
      const res = await BridgeAPI.getMesh();
      this._fetching = false;
      if (!this._root || !this._root.isConnected) return;
      if (res.error) {
        this._error = (res.error && res.error.error) || 'mesh unavailable';
        if (!this._mesh) this._renderBody();
        return;
      }
      this._error = null;
      this._mesh = res.data;
      this._renderBody();
    },

    _renderBody() {
      const el = this._root.querySelector('#mesh-content');
      if (!el) return;
      if (!this._mesh) {
        el.innerHTML = this._error
          ? BridgeUI.errorState('mesh unavailable — retrying', this._error)
          : BridgeUI.skeletons(2, 200);
        return;
      }
      const nodes = Array.isArray(this._mesh.nodes) ? this._mesh.nodes : [];
      if (nodes.length === 0) {
        el.innerHTML = BridgeUI.emptyState('No mesh peers — this node is running standalone');
        return;
      }
      el.innerHTML = this._topologyHtml(nodes) + this._peersHtml(nodes);
    },

    _topologyHtml(nodes) {
      const pos = layout(nodes);
      const connections = Array.isArray(this._mesh.connections) ? this._mesh.connections : [];
      const leaderId = nodes.find(n => n.is_leader)?.node_id || null;
      const links = connections.map(c => {
        const a = pos.get(c.from);
        const b = pos.get(c.to);
        if (!a || !b) return '';
        const leaderCls = leaderId && (c.from === leaderId || c.to === leaderId) ? ' mesh-link--leader' : '';
        return `<line class="mesh-link${leaderCls}" x1="${a.x}" y1="${a.y}" x2="${b.x}" y2="${b.y}" opacity="0.7"></line>`;
      }).join('');
      const localId = this._mesh.local_node_id;
      const groups = nodes.map(node => {
        const p = pos.get(node.node_id);
        const health = String(node.health || '').toLowerCase();
        const hCls = ['healthy', 'suspected', 'dead'].includes(health) ? ` mesh-node--${health}` : '';
        const lCls = node.is_leader ? ' mesh-node--leader' : '';
        const localRing = node.node_id === localId
          ? '<circle r="17" fill="none" stroke="#737373" stroke-dasharray="3 3"></circle>'
          : '';
        const tooltip = `${node.node_id != null ? node.node_id : '—'} · ${node.mesh_ip != null ? node.mesh_ip : '—'} · ${health || 'unknown'}`;
        return `
          <g class="mesh-node${hCls}${lCls}" transform="translate(${p.x} ${p.y})">
            ${localRing}
            <circle r="12"></circle>
            <text y="28" text-anchor="middle">${BridgeUI.esc(node.node_id != null ? node.node_id : '—')}</text>
            <title>${BridgeUI.esc(tooltip)}</title>
          </g>`;
      }).join('');
      return `<div class="mesh-canvas-wrap" style="margin-bottom:16px"><svg viewBox="0 0 800 460" role="img" aria-label="WireGuard mesh topology">${links}${groups}</svg></div>`;
    },

    _peersHtml(nodes) {
      const body = BridgeUI.table({
        columns: [
          { label: 'Node', primary: true, render: n => `<span class="mono">${BridgeUI.esc(n.node_id != null ? n.node_id : '—')}</span>${n.is_leader ? ' ' + BridgeUI.badge('LEADER', 'leader') : ''}` },
          { label: 'Status', render: n => `${BridgeUI.statusDot(n.health)} ${BridgeUI.healthBadge(n.health)}` },
          { label: 'Mesh IP', mono: true, render: n => BridgeUI.esc(n.mesh_ip != null ? n.mesh_ip : '—') },
          { label: 'Endpoint', mono: true, render: n => BridgeUI.esc(n.endpoint != null ? n.endpoint : '—') },
        ],
        rows: nodes,
        empty: 'No mesh peers — this node is running standalone',
      });
      return BridgeUI.panel({ title: 'Peers', flush: true, body });
    },
  };
})();
