// Requests: traffic totals and per-route request metrics.
(function () {
  'use strict';

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.requests = {
    title: 'Requests',
    _root: null,

    render(container, state) {
      this._root = container;
      container.innerHTML = `
        <div id="req-metrics">${BridgeUI.skeletons(1, 96)}</div>
        <div id="req-table">${BridgeUI.skeletons(1, 200)}</div>`;
      this.update(state);
    },

    update(state) {
      if (!this._root || !this._root.isConnected) return;
      const metricsEl = this._root.querySelector('#req-metrics');
      const tableEl = this._root.querySelector('#req-table');
      if (!metricsEl || !tableEl) return;

      if (!state.metrics) {
        if (state.errors.metrics) {
          metricsEl.innerHTML = '';
          tableEl.innerHTML = BridgeUI.errorState('metrics unavailable, retrying', state.errors.metrics);
        } else {
          metricsEl.innerHTML = BridgeUI.skeletons(1, 96);
          tableEl.innerHTML = BridgeUI.skeletons(1, 200);
        }
        return;
      }

      const routes = state.metrics.routes || {};
      let totalReq = 0;
      let totalErr = 0;
      const rows = Object.entries(routes).map(([host, m]) => {
        const stats = m || {};
        totalReq += stats.requests || 0;
        totalErr += stats.errors || 0;
        return { host, requests: stats.requests, errors: stats.errors, error_rate: stats.error_rate, p50_ms: stats.p50_ms, p95_ms: stats.p95_ms };
      }).sort((a, b) => (b.requests || 0) - (a.requests || 0));

      const rates = state.rates;
      metricsEl.innerHTML = BridgeUI.metricGrid([
        BridgeUI.metric({ label: 'Total Requests', value: BridgeUI.fmt.num(totalReq) }),
        BridgeUI.metric({ label: 'Errors', value: BridgeUI.fmt.num(totalErr), tone: totalErr > 0 ? 'danger' : '' }),
        BridgeUI.metric({ label: 'Requests/Sec', value: rates ? rates.rps.toFixed(1) : '–' }),
        BridgeUI.metric({ label: 'Error Rate', value: rates ? BridgeUI.fmt.pct(rates.errorRate) : '–', tone: rates ? (rates.errorRate > 0.01 ? 'danger' : rates.errorRate > 0 ? 'warning' : '') : '' }),
      ].join(''));

      const body = rows.length === 0
        ? BridgeUI.emptyState('No traffic recorded yet', 'Metrics accumulate as the proxy serves requests.')
        : BridgeUI.table({
            columns: [
              { label: 'Route', primary: true, render: r => `<span class="mono">${BridgeUI.esc(r.host)}</span>` },
              { label: 'Requests', num: true, render: r => BridgeUI.fmt.num(r.requests) },
              { label: 'Errors', num: true, render: r => BridgeUI.fmt.num(r.errors) },
              { label: 'Error Rate', render: r => {
                  const text = BridgeUI.fmt.pct(r.error_rate);
                  return (r.error_rate || 0) > 0.01 ? `<span style="color:var(--danger)">${text}</span>` : text;
                } },
              { label: 'P50', render: r => BridgeUI.fmt.ms(r.p50_ms) },
              { label: 'P95', render: r => BridgeUI.fmt.ms(r.p95_ms) },
            ],
            rows,
            empty: 'No traffic recorded yet',
          });
      tableEl.innerHTML = BridgeUI.panel({ title: 'Per-Route Traffic', flush: true, body });
    },

    destroy() {
      this._root = null;
    },
  };
})();
