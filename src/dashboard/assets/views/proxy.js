// Proxy: read-only proxy configuration & state (mode, listeners, UDP services).
(function () {
  'use strict';

  const MODE_LABELS = {
    Direct: 'L7 Direct',
    Handoff: 'L4 SNI Passthrough',
    SniPassthrough: 'L4 SNI Passthrough',
    Managed: 'Managed Coolify',
  };

  // Config values may be strings, numbers, booleans, or serialized structs:
  // render objects as JSON so nothing shows up as "[object Object]".
  function raw(v) {
    if (v === null || v === undefined) return v;
    return typeof v === 'object' ? JSON.stringify(v) : v;
  }

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.proxy = {
    title: 'Proxy',
    _root: null,
    _fetching: false,

    async render(container, state) {
      this._root = container;
      container.innerHTML = BridgeUI.skeletons(2, 150);
      if (state.config) {
        this._renderAll(state);
        return;
      }
      await this._fetchConfig(state);
    },

    update(state) {
      if (!this._root || !this._root.isConnected) return;
      if (!state.config && state.errors.config) {
        this._fetchConfig(state); // previous fetch errored: retry on poll ticks
        return;
      }
      if (state.config) this._renderAll(state);
    },

    destroy() {
      this._root = null;
    },

    async _fetchConfig(state) {
      if (this._fetching) return;
      this._fetching = true;
      const res = await BridgeAPI.getConfig();
      this._fetching = false;
      if (!this._root || !this._root.isConnected) return;
      if (res.error) {
        state.errors.config = (res.error && res.error.error) || 'config unavailable';
      } else {
        state.errors.config = null;
        state.config = res.data && res.data.config ? res.data.config : null;
      }
      this._renderAll(state);
    },

    _renderAll(state) {
      if (!this._root || !this._root.isConnected) return;
      let cfg = state.config;
      if (!cfg) {
        this._root.innerHTML = state.errors.config
          ? BridgeUI.errorState('config unavailable, retrying', state.errors.config)
          : BridgeUI.skeletons(2, 150);
        return;
      }
      // Tolerate the raw API envelope ({config: {...}}) in case it was stored unwrapped.
      if (cfg.config && typeof cfg.config === 'object' && !cfg.proxy && !cfg.udp_services) {
        cfg = cfg.config;
      }
      if (typeof cfg !== 'object' || Object.keys(cfg).length === 0) {
        this._root.innerHTML = BridgeUI.emptyState('No proxy configuration', 'The daemon returned an empty configuration.');
        return;
      }

      const proxy = cfg.proxy || {};
      const udp = cfg.udp_services || proxy.udp_services || [];
      const udpRows = Array.isArray(udp) ? udp : [];
      const listeners = Array.isArray(proxy.listeners) && proxy.listeners.length
        ? proxy.listeners.join(', ')
        : null;
      const mode = proxy.mode != null ? (MODE_LABELS[proxy.mode] || String(proxy.mode)) : null;
      const routesCount = state.status ? state.status.routes_count : null;

      const metrics = BridgeUI.metricGrid([
        BridgeUI.metric({ label: 'Mode', value: mode, tone: 'accent' }),
        BridgeUI.metric({ label: 'Listeners', value: listeners || '–' }),
        BridgeUI.metric({ label: 'Routes', value: routesCount != null ? BridgeUI.fmt.num(routesCount) : '–' }),
        BridgeUI.metric({ label: 'UDP Services', value: BridgeUI.fmt.num(udpRows.length) }),
      ].join(''));

      const listenerPanel = BridgeUI.panel({
        title: 'Listeners',
        body: BridgeUI.kvList([
          ['mode', raw(proxy.mode)],
          ['listeners', listeners],
          ['http_addr', raw(proxy.http_addr)],
          ['https_addr', raw(proxy.https_addr)],
          ['redirect_http', proxy.redirect_http != null ? String(proxy.redirect_http) : null],
          ['routing', raw(proxy.routing)],
          ['keep_alive_duration', raw(proxy.keep_alive_duration)],
        ]),
      });

      const udpPanel = BridgeUI.panel({
        title: 'UDP Services',
        flush: true,
        body: BridgeUI.table({
          columns: [
            { label: 'Listen Port', num: true, render: u => BridgeUI.fmt.num(u.listen_port) },
            { label: 'Upstream', mono: true, render: u => BridgeUI.esc(raw(u.upstream) != null ? raw(u.upstream) : '–') },
            { label: 'Node', mono: true, render: u => BridgeUI.esc(u.node_id != null ? u.node_id : '–') },
          ],
          rows: udpRows,
          empty: 'No UDP services',
        }),
      });

      this._root.innerHTML = metrics + listenerPanel + udpPanel;
    },
  };
})();
