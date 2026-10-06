// Cluster view: read-only cluster membership (poll-driven via state) and
// cluster configuration ([node], [[seeds]], [[nodes]] from GET /api/v1/config).
'use strict';
(function () {
  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.cluster = {
    title: 'Cluster',

    _root: null,
    _config: null,
    _configError: null,

    async render(container, state) {
      const root = (this._root = container);
      this._config = null;
      this._configError = null;

      container.innerHTML = `
        <div id="cl-metrics">${BridgeUI.skeletons(1, 84)}</div>
        ${BridgeUI.panel({ title: 'Members', body: '<div id="cl-members"></div>', flush: true })}
        ${BridgeUI.panel({ title: 'Node Configuration', body: '<div id="cl-node"></div>' })}
        ${BridgeUI.panel({ title: 'Seeds', body: '<div id="cl-seeds"></div>', flush: true })}
        ${BridgeUI.panel({ title: 'Known Nodes', body: '<div id="cl-known"></div>', flush: true })}`;
      this._renderState(state);

      const res = await BridgeAPI.getConfig();
      if (!root.isConnected || this._root !== root) return;
      if (res.error) this._configError = res.error.error || 'Configuration unavailable';
      else this._config = (res.data && res.data.config) || {};
      this._renderConfigPanels();
    },

    update(state) {
      if (this._root && this._root.isConnected) this._renderState(state);
    },

    destroy() {
      this._root = null;
    },

    /* ------------------------------------------------------------ state-driven */

    _renderState(state) {
      const root = this._root;
      if (!root) return;

      const m = root.querySelector('#cl-metrics');
      if (m) {
        const st = state.status;
        if (!st && state.errors.status) {
          m.innerHTML = BridgeUI.errorState('Status unavailable', state.errors.status);
        } else if (!st) {
          m.innerHTML = BridgeUI.skeletons(1, 84);
        } else {
          const conv = st.convergence_state || '';
          m.innerHTML = BridgeUI.metricGrid(
            BridgeUI.metric({ label: 'Node ID', value: st.node_id || '–', mono: true }) +
            BridgeUI.metric({
              label: 'Role',
              value: st.is_leader ? 'LEADER' : 'PEER',
              tone: st.is_leader ? 'accent' : '',
            }) +
            (st.leader_id
              ? BridgeUI.metric({ label: 'Leader', value: st.leader_id, mono: true })
              : BridgeUI.metric({ label: 'Leader', value: 'No Leader', tone: 'danger' })) +
            BridgeUI.metric({ label: 'Nodes', value: BridgeUI.fmt.num(st.node_count) }) +
            BridgeUI.metric({
              label: 'Convergence',
              value: conv || '–',
              tone: conv === 'converged' ? 'success' : (conv ? 'warning' : ''),
            })
          );
        }
      }

      const members = root.querySelector('#cl-members');
      if (members) {
        if (!state.nodes && state.errors.nodes) {
          members.innerHTML = BridgeUI.errorState('Nodes unavailable', state.errors.nodes);
        } else if (!state.nodes) {
          members.innerHTML = `<div style="padding:16px">${BridgeUI.skeletons(2, 44)}</div>`;
        } else {
          members.innerHTML = BridgeUI.table({
            columns: [
              {
                label: 'Node', mono: true, primary: true,
                render: n => BridgeUI.esc(n.node_id || '–') +
                  (n.is_leader ? ` ${BridgeUI.badge('LEADER', 'leader')}` : ''),
              },
              {
                label: 'Status',
                render: n => `${BridgeUI.statusDot(n.health)} ${BridgeUI.healthBadge(n.health)}`,
              },
              { label: 'Mesh IP', mono: true, render: n => BridgeUI.esc(n.mesh_ip || '–') },
              { label: 'Endpoint', mono: true, render: n => BridgeUI.esc(n.endpoint || '–') },
              { label: 'Priority', num: true, render: n => BridgeUI.fmt.num(n.priority) },
            ],
            rows: state.nodes,
            empty: 'No cluster peers, standalone mode',
          });
        }
      }
    },

    /* ------------------------------------------------------------ config-driven */

    _renderConfigPanels() {
      const root = this._root;
      if (!root) return;

      if (this._configError) {
        for (const sel of ['#cl-node', '#cl-seeds', '#cl-known']) {
          const el = root.querySelector(sel);
          if (el) el.innerHTML = BridgeUI.errorState('Configuration unavailable', this._configError);
        }
        return;
      }

      const cfg = this._config || {};

      const nodeEl = root.querySelector('#cl-node');
      if (nodeEl) {
        const node = cfg.node;
        if (!node) {
          nodeEl.innerHTML = BridgeUI.emptyState('No [node] section, running standalone');
        } else {
          nodeEl.innerHTML = BridgeUI.kvList([
            ['id', node.id],
            ['mesh_ip', node.mesh_ip],
            ['endpoint', node.endpoint],
            ['listen_port', node.listen_port !== undefined && node.listen_port !== null
              ? String(node.listen_port) : undefined],
            ['priority', node.priority !== undefined && node.priority !== null
              ? String(node.priority) : undefined],
            ['public_key', node.public_key ? this._truncate(node.public_key) : undefined],
          ]);
        }
      }

      const seedsEl = root.querySelector('#cl-seeds');
      if (seedsEl) {
        const seeds = Array.isArray(cfg.seeds) ? cfg.seeds : [];
        seedsEl.innerHTML = BridgeUI.table({
          columns: [
            {
              label: 'Endpoint', mono: true, primary: true,
              render: s => BridgeUI.esc(typeof s === 'string' ? s : (s && s.endpoint) || '–'),
            },
          ],
          rows: seeds,
          empty: 'No seed nodes configured',
        });
      }

      const knownEl = root.querySelector('#cl-known');
      if (knownEl) {
        // registry::Node serializes as node_id/address; the TOML aliases
        // id/endpoint are accepted as fallbacks.
        const nodes = Array.isArray(cfg.nodes) ? cfg.nodes : [];
        knownEl.innerHTML = BridgeUI.table({
          columns: [
            {
              label: 'Node ID', mono: true, primary: true,
              render: n => BridgeUI.esc((n && (n.node_id || n.id)) || '–'),
            },
            {
              label: 'Endpoint', mono: true,
              render: n => BridgeUI.esc((n && (n.address || n.endpoint)) || '–'),
            },
          ],
          rows: nodes,
          empty: 'No static nodes configured',
        });
      }
    },

    _truncate(key) {
      const s = String(key);
      return s.length > 16 ? `${s.slice(0, 16)}…` : s;
    },
  };
})();
