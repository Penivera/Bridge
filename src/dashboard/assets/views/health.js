// Health view — daemon liveness (GET /health, round-trip measured client-side)
// plus an honest subsystem table: only states derivable from real data
// (status, loaded config) are shown — no invented runtime health.
'use strict';
(function () {
  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.health = {
    title: 'Health',

    _root: null,
    _health: null,
    _healthError: null,
    _rtt: null,
    _config: null,
    _configError: null,

    async render(container, state) {
      const root = (this._root = container);
      container.innerHTML = BridgeUI.skeletons(3, 140);

      // Time the health probe specifically, not the parallel config fetch.
      const t0 = performance.now();
      const healthP = BridgeAPI.getHealth().then(res => {
        this._rtt = performance.now() - t0;
        return res;
      });
      const [health, cfg] = await Promise.all([healthP, BridgeAPI.getConfig()]);
      if (!root.isConnected || this._root !== root) return;

      if (health.error) {
        this._health = null;
        this._healthError = health.error.error || 'Health check failed';
      } else {
        this._health = health.data || {};
        this._healthError = null;
      }
      if (cfg.error) {
        this._config = null;
        this._configError = cfg.error.error || 'Configuration unavailable';
      } else {
        this._config = (cfg.data && cfg.data.config) || {};
        this._configError = null;
      }
      this._renderAll(state);
    },

    update(state) {
      // Refresh the poll-driven parts (daemon panel, version, cluster row)
      // from the latest state; the probe results stay from the last render.
      if (this._root && this._root.isConnected && (this._health || this._healthError)) {
        this._renderAll(state);
      }
    },

    destroy() {
      this._root = null;
    },

    /* ------------------------------------------------------------ internals */

    _renderAll(state) {
      const root = this._root;
      if (!root) return;
      root.innerHTML = `
        ${this._metricsHtml(state)}
        ${BridgeUI.panel({ title: 'Daemon', body: this._daemonBody(state) })}
        ${BridgeUI.panel({
          title: 'Subsystems',
          actions: '<span class="muted small">Configured state — not a runtime probe.</span>',
          body: this._subsystemsBody(state),
        })}`;
    },

    _metricsHtml(state) {
      const ok = Boolean(this._health) && this._health.status === 'ok';
      const liveness = this._healthError
        ? BridgeUI.metric({ label: 'Liveness', value: 'error', tone: 'danger', sub: this._healthError })
        : BridgeUI.metric({
            label: 'Liveness',
            value: ok ? 'ok' : String((this._health && this._health.status) || '—'),
            tone: ok ? 'success' : 'danger',
          });
      return BridgeUI.metricGrid(
        liveness +
        BridgeUI.metric({
          label: 'Response',
          value: this._rtt !== null ? BridgeUI.fmt.ms(this._rtt) : '—',
          mono: true,
        }) +
        BridgeUI.metric({
          label: 'Uptime',
          value: this._health ? BridgeUI.fmt.uptime(this._health.uptime_secs) : '—',
        }) +
        BridgeUI.metric({
          label: 'Version',
          value: state.status && state.status.version ? `v${state.status.version}` : '—',
          mono: true,
        })
      );
    },

    _daemonBody(state) {
      const s = state.status;
      if (!s) {
        if (state.errors.status) return BridgeUI.errorState('Status unavailable', state.errors.status);
        return BridgeUI.skeletons(1, 120);
      }
      return BridgeUI.kvList([
        ['node_id', s.node_id !== undefined && s.node_id !== null ? String(s.node_id) : '—'],
        ['version', s.version !== undefined && s.version !== null ? String(s.version) : '—'],
        ['convergence_state', s.convergence_state || '—'],
        ['node_count', s.node_count !== undefined && s.node_count !== null ? String(s.node_count) : '—'],
        ['routes_count', s.routes_count !== undefined && s.routes_count !== null ? String(s.routes_count) : '—'],
        ['replicas_count', s.replicas_count !== undefined && s.replicas_count !== null ? String(s.replicas_count) : '—'],
        ['leader_id', s.leader_id || '—'],
      ]);
    },

    _subsystemsBody(state) {
      const flagBadge = (flag) => flag
        ? BridgeUI.badge('enabled', 'accent')
        : BridgeUI.badge('disabled', 'neutral');
      // null = config slice unavailable — honest placeholder, not a guess.
      const cfgFlag = (fn) => {
        if (this._configError || !this._config) return '<span class="muted">—</span>';
        return flagBadge(Boolean(fn(this._config)));
      };

      const conv = state.status && state.status.convergence_state;
      const rows = [
        { name: 'Dashboard', stateHtml: BridgeUI.badge('serving', 'accent') },
        {
          name: 'Cluster',
          stateHtml: conv
            ? BridgeUI.badge(conv, conv === 'converged' ? 'accent' : 'neutral')
            : '<span class="muted">—</span>',
        },
        { name: 'Discovery', stateHtml: cfgFlag(c => c.discovery && c.discovery.enabled) },
        { name: 'IPC', stateHtml: cfgFlag(c => c.ipc && c.ipc.enabled) },
        { name: 'Telemetry', stateHtml: cfgFlag(c => c.enable_telemetry) },
      ];
      return BridgeUI.table({
        columns: [
          { label: 'Subsystem', primary: true, render: r => BridgeUI.esc(r.name) },
          { label: 'State', render: r => r.stateHtml },
        ],
        rows,
      });
    },
  };
})();
