// Handoffs view — ingress failover mode, tier documentation, handoff config,
// and the two real failover operations exposed by the daemon
// (POST /api/v1/replicate, POST /api/v1/failback).
'use strict';
(function () {
  // Tier numbering follows src/core/config.rs ("Tier 1: none, Tier 2: dns,
  // Tier 3a: tunnel, Tier 3b: floating_ip"). Keys match HandoffMode's
  // snake_case serialization.
  const TIERS = [
    { tier: '1', key: 'none', name: 'None', when: 'Single node, no failover' },
    { tier: '2', key: 'dns', name: 'DNS', when: 'Health-checked DNS records follow the leader' },
    { tier: '3a', key: 'tunnel', name: 'Tunnel', when: 'Cloudflare Tunnel held by the leader' },
    { tier: '3b', key: 'floating_ip', name: 'Floating IP', when: 'Provider IP reassigned on leader change' },
  ];

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.handoffs = {
    title: 'Handoffs',

    _root: null,
    _handoff: null,
    _handoffError: null,
    _config: null,
    _configError: null,

    async render(container, state) {
      const root = (this._root = container);
      this._handoff = null;
      this._handoffError = null;
      this._config = null;
      this._configError = null;

      container.innerHTML = `
        <div id="ho-metrics">${BridgeUI.skeletons(1, 84)}</div>
        ${BridgeUI.panel({ title: 'Failover Tiers', body: '<div id="ho-tiers"></div>' })}
        ${BridgeUI.panel({ title: 'Handoff Configuration', body: '<div id="ho-config"></div>' })}
        ${BridgeUI.panel({ title: 'Operations', body: this._opsBody() })}`;
      this._wireOps();

      const [ho, cfg] = await Promise.all([BridgeAPI.getHandoff(), BridgeAPI.getConfig()]);
      if (!root.isConnected || this._root !== root) return;

      if (ho.error) this._handoffError = ho.error.error || 'Handoff status unavailable';
      else this._handoff = ho.data || {};
      if (cfg.error) this._configError = cfg.error.error || 'Configuration unavailable';
      else this._config = (cfg.data && cfg.data.config) || {};

      this._renderMetrics(state);
      this._renderTiers();
      this._renderConfig();
    },

    update(state) {
      // Only the metric strip is poll-driven; it holds no user input.
      if (this._root && this._root.isConnected && (this._handoff || this._handoffError)) {
        this._renderMetrics(state);
      }
    },

    destroy() {
      this._root = null;
    },

    /* ------------------------------------------------------------ panels */

    _renderMetrics(state) {
      const el = this._root && this._root.querySelector('#ho-metrics');
      if (!el) return;
      const st = state.status;
      if (this._handoffError) {
        el.innerHTML = BridgeUI.errorState('Handoff status unavailable', this._handoffError);
        return;
      }
      const mode = String((this._handoff && this._handoff.mode) || 'none').toUpperCase();
      el.innerHTML = BridgeUI.metricGrid(
        BridgeUI.metric({
          label: 'Mode',
          value: mode,
          tone: mode === 'NONE' ? '' : 'accent',
        }) +
        BridgeUI.metric({
          label: 'Replicas',
          value: st && st.replicas_count !== undefined && st.replicas_count !== null
            ? BridgeUI.fmt.num(st.replicas_count)
            : '—',
        }) +
        BridgeUI.metric({
          label: 'Convergence',
          value: st ? (st.convergence_state || '—') : '—',
        })
      );
    },

    _renderTiers() {
      const el = this._root && this._root.querySelector('#ho-tiers');
      if (!el) return;
      if (this._handoffError) {
        el.innerHTML = BridgeUI.errorState('Handoff status unavailable', this._handoffError);
        return;
      }
      const mode = String((this._handoff && this._handoff.mode) || 'none');
      el.innerHTML = BridgeUI.table({
        columns: [
          { label: 'Tier', mono: true, render: t => BridgeUI.esc(t.tier) },
          {
            label: 'Name', primary: true,
            render: t => BridgeUI.esc(t.name) +
              (t.key === mode ? ` ${BridgeUI.badge('active', 'accent')}` : ''),
          },
          { label: 'When it applies', render: t => BridgeUI.esc(t.when) },
        ],
        rows: TIERS,
      });
    },

    _renderConfig() {
      const el = this._root && this._root.querySelector('#ho-config');
      if (!el) return;
      if (this._configError) {
        el.innerHTML = BridgeUI.errorState('Configuration unavailable', this._configError);
        return;
      }
      const h = this._config && this._config.handoff;
      if (!h) {
        el.innerHTML = BridgeUI.emptyState('No handoff configuration');
        return;
      }
      el.innerHTML = BridgeUI.kvList([
        ['mode', h.mode !== undefined && h.mode !== null ? String(h.mode) : '—'],
        ['tunnel', h.tunnel ? 'yes' : 'no'],
        ['dns', h.dns ? 'yes' : 'no'],
      ]);
    },

    /* ------------------------------------------------------------ operations */

    _opsBody() {
      const op = (o) => `
        <div class="form-section">
          <div class="form-section-title">${o.title}</div>
          <div class="form-grid">
            ${BridgeUI.field({
              label: o.fieldLabel, name: o.input, mono: true,
              placeholder: o.placeholder, hint: o.hint,
            })}
            <div class="field">
              <label class="field-label">&nbsp;</label>
              <div><button class="btn ${o.btnClass}" id="${o.btn}" type="button">${o.btnLabel}</button></div>
            </div>
          </div>
          <div id="${o.result}"></div>
        </div>`;
      return op({
        title: 'Trigger replication',
        fieldLabel: 'Node ID',
        input: 'ho-rep-node',
        placeholder: 'vm-02',
        hint: 'Spawn replica workloads on the given node.',
        btn: 'ho-rep-btn',
        btnClass: '',
        btnLabel: 'Trigger',
        result: 'ho-rep-result',
      }) + op({
        title: 'Trigger failback',
        fieldLabel: 'Domain',
        input: 'ho-fb-domain',
        placeholder: 'example.com',
        hint: 'Stop replica workloads and fail the domain back to its origin.',
        btn: 'ho-fb-btn',
        btnClass: 'btn--danger',
        btnLabel: 'Trigger failback',
        result: 'ho-fb-result',
      });
    },

    _wireOps() {
      const root = this._root;
      root.querySelector('#ho-rep-btn').addEventListener('click', async (e) => {
        const btn = e.currentTarget;
        const input = root.querySelector('[name="ho-rep-node"]');
        const id = (input.value || '').trim();
        const fieldEl = input.closest('.field');
        if (!id) {
          BridgeUI.setFieldError(fieldEl, 'Node ID is required');
          return;
        }
        BridgeUI.setFieldError(fieldEl, '');
        const ok = await BridgeUI.confirm({
          title: 'Trigger replication',
          body: `Spawn replica workloads on node <span class="mono">${BridgeUI.esc(id)}</span>?`,
          confirmLabel: 'Trigger',
        });
        if (!ok || !root.isConnected || this._root !== root) return;
        BridgeUI.setLoading(btn, true);
        const res = await BridgeAPI.replicate(id);
        if (!root.isConnected || this._root !== root) return;
        BridgeUI.setLoading(btn, false);
        this._opResult('#ho-rep-result', res);
      });

      root.querySelector('#ho-fb-btn').addEventListener('click', async (e) => {
        const btn = e.currentTarget;
        const input = root.querySelector('[name="ho-fb-domain"]');
        const domain = (input.value || '').trim();
        const fieldEl = input.closest('.field');
        if (!domain) {
          BridgeUI.setFieldError(fieldEl, 'Domain is required');
          return;
        }
        BridgeUI.setFieldError(fieldEl, '');
        const ok = await BridgeUI.confirm({
          title: 'Trigger failback',
          body: `Stop replica workloads and fail back <span class="mono">${BridgeUI.esc(domain)}</span>?`,
          confirmLabel: 'Fail back',
          danger: true,
        });
        if (!ok || !root.isConnected || this._root !== root) return;
        BridgeUI.setLoading(btn, true);
        const res = await BridgeAPI.failback(domain);
        if (!root.isConnected || this._root !== root) return;
        BridgeUI.setLoading(btn, false);
        this._opResult('#ho-fb-result', res);
      });
    },

    _opResult(selector, res) {
      const el = this._root && this._root.querySelector(selector);
      if (!el) return;
      if (!res.error) {
        const msg = (res.data && res.data.message) || 'Done';
        el.innerHTML = `<div class="form-banner form-banner--success">${BridgeUI.esc(msg)}</div>`;
        return;
      }
      let msg = res.error.error || 'Request failed';
      if (res.status === 503) {
        // The endpoint is only wired when the failover duplicator runs.
        msg = `${msg} — the failover duplicator is not running`;
      }
      el.innerHTML = `<div class="form-banner form-banner--error">${BridgeUI.esc(msg)}</div>`;
    },
  };
})();
