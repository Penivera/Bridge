// Overview — cluster at a glance: health metrics, node & route tables, recent events.
(function () {
  'use strict';

  const EVENT_LIMIT = 8;

  function sevClass(severity) {
    const s = String(severity || 'info').toLowerCase();
    if (s === 'critical' || s === 'error') return 'critical';
    if (s === 'warning') return 'warning';
    return 'info';
  }

  function eventKey(e) {
    return `${e.timestamp}|${e.event_type}|${e.message}`;
  }

  function eventRow(e, isNew) {
    const sev = sevClass(e.severity);
    return `
      <div class="event-row${isNew ? ' event-row--new' : ''}">
        <div class="event-time">${BridgeUI.fmt.time(e.timestamp)} · ${BridgeUI.fmt.ago(e.timestamp)}</div>
        <div class="event-sev-col"><span class="severity-bar severity-bar--${sev}"></span>${BridgeUI.esc(e.event_type || '—')}</div>
        <div class="event-node">${BridgeUI.esc(e.node_id || '—')}</div>
        <div class="event-message">${BridgeUI.esc(e.message || '')}</div>
      </div>`;
  }

  function clusterHealth(state) {
    if (!Array.isArray(state.nodes)) return { label: null, tone: '' };
    const dead = state.nodes.filter(n => String(n.health).toLowerCase() === 'dead').length;
    const suspected = state.nodes.filter(n => String(n.health).toLowerCase() === 'suspected').length;
    const noLeader = ((state.status && state.status.node_count) || 1) > 1 && !(state.status && state.status.leader_id);
    if (dead > 0) return { label: 'Critical', tone: 'danger' };
    if (suspected > 0 || noLeader) return { label: 'Degraded', tone: 'warning' };
    return { label: 'Healthy', tone: 'success' };
  }

  function routeMetric(state, domain) {
    return state.metrics && state.metrics.routes ? state.metrics.routes[domain] : null;
  }

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.overview = {
    title: 'Overview',
    _root: null,
    _events: [],

    async render(container, state) {
      this._root = container;
      container.innerHTML = `
        <div id="ov-metrics">${BridgeUI.skeletons(1, 96)}</div>
        <div id="ov-nodes">${BridgeUI.skeletons(1, 180)}</div>
        <div id="ov-routes">${BridgeUI.skeletons(1, 160)}</div>
        <div id="ov-events">${BridgeUI.skeletons(1, 140)}</div>`;
      this.update(state);

      const res = await BridgeAPI.getEvents(1, EVENT_LIMIT);
      if (!this._root || !this._root.isConnected) return;
      if (!res.error && res.data && Array.isArray(res.data.events)) {
        this._merge(res.data.events);
        this._renderEvents();
      }
    },

    update(state) {
      if (!this._root || !this._root.isConnected) return;
      this._renderMetrics(state);
      this._renderNodes(state);
      this._renderRoutes(state);
      this._merge(state.events || []);
      this._renderEvents();
    },

    onEvent(event) {
      if (!this._root || !this._root.isConnected) return;
      const key = eventKey(event);
      if (this._events.some(e => eventKey(e) === key)) return;
      this._events.unshift(event);
      this._events = this._events.slice(0, EVENT_LIMIT);
      this._renderEvents(key);
    },

    destroy() {
      this._root = null;
    },

    _merge(list) {
      const seen = new Set(this._events.map(eventKey));
      for (const e of list) {
        const k = eventKey(e);
        if (!seen.has(k)) {
          seen.add(k);
          this._events.push(e);
        }
      }
      this._events.sort((a, b) => (Date.parse(b.timestamp) || 0) - (Date.parse(a.timestamp) || 0));
      this._events = this._events.slice(0, EVENT_LIMIT);
    },

    _renderMetrics(state) {
      const el = this._root.querySelector('#ov-metrics');
      if (!el) return;
      const s = state.status;
      if (!s) {
        el.innerHTML = state.errors.status
          ? BridgeUI.errorState('status unavailable — retrying', state.errors.status)
          : BridgeUI.skeletons(1, 96);
        return;
      }
      const cluster = clusterHealth(state);
      const healthy = Array.isArray(state.nodes)
        ? state.nodes.filter(n => String(n.health).toLowerCase() === 'healthy').length
        : null;
      const noLeader = !s.leader_id && (s.node_count || 1) > 1;
      const rates = state.rates;
      el.innerHTML = BridgeUI.metricGrid([
        BridgeUI.metric({ label: 'Cluster', value: cluster.label, tone: cluster.tone, sub: s.convergence_state || '' }),
        BridgeUI.metric({ label: 'Nodes', value: BridgeUI.fmt.num(s.node_count), sub: healthy !== null ? `${healthy} healthy` : '' }),
        BridgeUI.metric({ label: 'Leader', value: s.leader_id || 'No Leader', mono: true, tone: noLeader ? 'danger' : '', sub: s.is_leader ? 'this node' : 'peer' }),
        BridgeUI.metric({ label: 'Dashboard master', value: s.is_dashboard_master ? 'this node' : (s.leader_id || 'none'), mono: true, tone: s.is_dashboard_master ? 'accent' : '', sub: 'public ingress owner' }),
        BridgeUI.metric({ label: 'Mesh', value: s.convergence_state, tone: s.convergence_state === 'converged' ? 'success' : 'warning', sub: `${s.node_count != null ? s.node_count : '—'} nodes` }),
        BridgeUI.metric({ label: 'Requests', value: rates ? `${rates.rps.toFixed(1)} req/s` : '—', sub: 'rolling avg' }),
        BridgeUI.metric({ label: 'Error Rate', value: rates ? BridgeUI.fmt.pct(rates.errorRate) : '—', tone: rates ? (rates.errorRate > 0.01 ? 'danger' : rates.errorRate > 0 ? 'warning' : '') : '' }),
        BridgeUI.metric({ label: 'Routes', value: BridgeUI.fmt.num(s.routes_count), sub: `${s.replicas_count != null ? s.replicas_count : '—'} replicas` }),
      ].join(''));
    },

    _renderNodes(state) {
      const el = this._root.querySelector('#ov-nodes');
      if (!el) return;
      let body;
      if (!Array.isArray(state.nodes)) {
        body = state.errors.nodes
          ? BridgeUI.errorState('nodes unavailable — retrying', state.errors.nodes)
          : BridgeUI.skeletons(1, 140);
      } else {
        body = BridgeUI.table({
          columns: [
            { label: 'Node', primary: true, render: n => `<span class="mono">${BridgeUI.esc(n.node_id != null ? n.node_id : '—')}</span>${n.is_leader ? ' ' + BridgeUI.badge('LEADER', 'leader') : ''}` },
            { label: 'Status', render: n => `${BridgeUI.statusDot(n.health)} ${BridgeUI.esc(n.health || 'unknown')}` },
            { label: 'Mesh IP', mono: true, render: n => BridgeUI.esc(n.mesh_ip != null ? n.mesh_ip : '—') },
            { label: 'Endpoint', mono: true, render: n => BridgeUI.esc(n.endpoint != null ? n.endpoint : '—') },
            { label: 'Priority', num: true, render: n => BridgeUI.fmt.num(n.priority) },
            { label: 'Uptime', render: n => BridgeUI.fmt.uptime(n.uptime_secs) },
          ],
          rows: state.nodes,
          empty: 'No nodes — running standalone',
        });
      }
      el.innerHTML = BridgeUI.panel({ title: 'Node Health', flush: true, body });
    },

    _renderRoutes(state) {
      const el = this._root.querySelector('#ov-routes');
      if (!el) return;
      let body;
      if (!Array.isArray(state.routes)) {
        body = state.errors.routes
          ? BridgeUI.errorState('routes unavailable — retrying', state.errors.routes)
          : BridgeUI.skeletons(1, 140);
      } else {
        body = BridgeUI.table({
          columns: [
            { label: 'Hostname', primary: true, render: r => `<span class="mono">${BridgeUI.esc(r.domain != null ? r.domain : '—')}</span>` },
            { label: 'Destination', mono: true, render: r => BridgeUI.esc(r.node_id != null ? r.node_id : '—') },
            { label: 'Backend', mono: true, render: r => BridgeUI.esc(r.target_addr != null ? r.target_addr : '—') },
            { label: 'Targets', render: r => `${BridgeUI.fmt.num(r.targets_count)}${r.has_hash_ring ? ' ' + BridgeUI.badge('hash ring', 'accent') : ''}` },
            { label: 'Requests', render: r => { const m = routeMetric(state, r.domain); return BridgeUI.fmt.num(m && m.requests); } },
            { label: 'P50', render: r => { const m = routeMetric(state, r.domain); return BridgeUI.fmt.ms(m && m.p50_ms); } },
            { label: 'P95', render: r => { const m = routeMetric(state, r.domain); return BridgeUI.fmt.ms(m && m.p95_ms); } },
          ],
          rows: state.routes,
          empty: 'No routes configured',
        });
      }
      el.innerHTML = BridgeUI.panel({ title: 'Route Health', flush: true, body });
    },

    _renderEvents(newKey) {
      const el = this._root.querySelector('#ov-events');
      if (!el) return;
      const rows = this._events.slice(0, EVENT_LIMIT);
      const body = rows.length
        ? `<div class="event-list">${rows.map((e, i) => eventRow(e, i === 0 && newKey != null && eventKey(e) === newKey)).join('')}</div>`
        : BridgeUI.emptyState('No events yet');
      el.innerHTML = BridgeUI.panel({ title: 'Recent Events', flush: true, body });
    },
  };
})();
