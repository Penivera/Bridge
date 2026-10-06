// Events view — operational event stream.
// Server-paginated history (GET /api/v1/events, newest-first) merged with the
// live WS buffer in state.events; severity filters client-side, event type
// filters server-side. currentPage / currentSeverity / currentType are
// initialized once here (never in render) so they survive view re-entry.
'use strict';
(function () {
  const SEVERITIES = [
    { key: 'ALL', label: 'All' },
    { key: 'info', label: 'Info' },
    { key: 'warning', label: 'Warning' },
    { key: 'critical', label: 'Critical' },
  ];
  const TYPES = ['', 'NODE_JOINED', 'NODE_DEAD'];
  const SEV_VARIANT = { info: 'info', warning: 'warning', critical: 'critical' };

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.events = {
    title: 'Events',

    perPage: 50,
    currentPage: 1,
    currentSeverity: 'ALL',
    currentType: '',

    _root: null,
    _serverEvents: null,   // events of the current server page
    _total: 0,             // server-reported total matching the type filter
    _fetchError: null,
    _loadedOnce: false,    // gates skeletons: only before first successful load

    async render(container, state) {
      const root = (this._root = container);
      container.innerHTML = this._shell();
      this._wire(state);

      if (this._loadedOnce && this._serverEvents) {
        this._renderList(state); // stale-while-revalidate on re-entry
      } else {
        const list = root.querySelector('#ev-list');
        if (list) list.innerHTML = BridgeUI.skeletons(3, 48);
      }
      await this._fetch(state, root);
    },

    update() {
      // No-op by design: the list refreshes on render, on filter/pagination
      // changes, and via live WS prepends. Re-rendering every poll tick would
      // replay row animations and fight the operator's scroll position.
    },

    onEvent(event, state) {
      const root = this._root;
      if (!root || !root.isConnected || this.currentPage !== 1) return;
      if (!this._passesFilters(event)) return;
      this._total += 1;
      const tbody = root.querySelector('#ev-tbody');
      if (!tbody) {
        // Empty/error placeholder is showing — rebuild normally; the event is
        // already in state.events (app.js unshifts before onEvent fires).
        this._renderList(state);
        return;
      }
      tbody.insertAdjacentHTML('afterbegin', this._rowHtml(event, true));
      while (tbody.rows.length > this.perPage) tbody.deleteRow(tbody.rows.length - 1);
      this._updateMeta();
    },

    destroy() {
      this._root = null;
    },

    /* ------------------------------------------------------------ internals */

    _shell() {
      const sevButtons = SEVERITIES.map(s =>
        `<button class="filter-btn${this.currentSeverity === s.key ? ' active' : ''}" data-sev="${s.key}" type="button">${s.label}</button>`
      ).join('');
      const typeOptions = TYPES.map(t =>
        `<option value="${t}"${this.currentType === t ? ' selected' : ''}>${t || 'All types'}</option>`
      ).join('');
      return BridgeUI.panel({
        title: 'Event Stream',
        actions: `
          <div class="filter-bar">
            ${sevButtons}
            <select class="field-select" id="ev-type" style="width:auto">${typeOptions}</select>
            <span class="muted small" id="ev-count"></span>
          </div>`,
        flush: true,
        body: `
          <div id="ev-list"></div>
          <div class="pagination">
            <button class="btn btn--sm" id="ev-prev" type="button">Prev</button>
            <span class="page-indicator" id="ev-page-info">Page ${this.currentPage}</span>
            <button class="btn btn--sm" id="ev-next" type="button">Next</button>
          </div>`,
      });
    },

    _wire(state) {
      const root = this._root;
      root.querySelectorAll('[data-sev]').forEach(btn => {
        btn.addEventListener('click', () => {
          this.currentSeverity = btn.dataset.sev;
          root.querySelectorAll('[data-sev]').forEach(b =>
            b.classList.toggle('active', b === btn));
          this._renderList(state); // client-side filter — no refetch
        });
      });
      const typeSel = root.querySelector('#ev-type');
      if (typeSel) typeSel.addEventListener('change', () => {
        this.currentType = typeSel.value;
        this.currentPage = 1;
        this._fetch(state, root);
      });
      const prev = root.querySelector('#ev-prev');
      if (prev) prev.addEventListener('click', () => {
        if (this.currentPage > 1) {
          this.currentPage -= 1;
          this._fetch(state, root);
        }
      });
      const next = root.querySelector('#ev-next');
      if (next) next.addEventListener('click', () => {
        this.currentPage += 1;
        this._fetch(state, root);
      });
    },

    async _fetch(state, root) {
      const res = await BridgeAPI.getEvents(this.currentPage, this.perPage, this.currentType || null);
      if (!root.isConnected || this._root !== root) return;
      if (res.error) {
        this._fetchError = res.error.error || 'Events request failed';
        if (!this._serverEvents || this._serverEvents.length === 0) {
          // Nothing else to show — surface the error in place of the list.
          this._renderList(state);
        }
        this._updateMeta();
        return;
      }
      this._fetchError = null;
      this._loadedOnce = true;
      this._serverEvents = (res.data && res.data.events) || [];
      this._total = res.data && typeof res.data.total === 'number'
        ? res.data.total
        : this._serverEvents.length;
      this._renderList(state);
    },

    _passesFilters(event) {
      if (this.currentType && event.event_type !== this.currentType) return false;
      if (this.currentSeverity !== 'ALL' &&
          String(event.severity || 'info').toLowerCase() !== this.currentSeverity) return false;
      return true;
    },

    _displayEvents(state) {
      let events = this._serverEvents || [];
      if (this.currentPage === 1) {
        // Merge the live WS buffer (same type filter applied) with the server
        // page; dedupe, newest-first, capped at one page.
        const ws = (state.events || [])
          .filter(e => !this.currentType || e.event_type === this.currentType);
        const seen = new Set();
        events = ws.concat(events).filter(e => {
          const k = this._key(e);
          if (seen.has(k)) return false;
          seen.add(k);
          return true;
        });
        events.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));
        events = events.slice(0, this.perPage);
      }
      if (this.currentSeverity !== 'ALL') {
        events = events.filter(e =>
          String(e.severity || 'info').toLowerCase() === this.currentSeverity);
      }
      return events;
    },

    _renderList(state) {
      const root = this._root;
      const list = root && root.querySelector('#ev-list');
      if (!list) return;
      const events = this._displayEvents(state);
      if (events.length === 0) {
        list.innerHTML = this._fetchError
          ? BridgeUI.errorState('Events unavailable', this._fetchError)
          : BridgeUI.emptyState('No events yet');
      } else {
        list.innerHTML = `
          <div class="table-wrap">
            <table class="data-table">
              <thead><tr>
                <th>Time</th><th>Severity</th><th>Type</th><th>Node</th><th>Message</th>
              </tr></thead>
              <tbody id="ev-tbody">${events.map(e => this._rowHtml(e, false)).join('')}</tbody>
            </table>
          </div>`;
      }
      this._updateMeta();
    },

    _updateMeta() {
      const root = this._root;
      if (!root) return;
      const tbody = root.querySelector('#ev-tbody');
      const count = root.querySelector('#ev-count');
      if (count) count.textContent = `${tbody ? tbody.rows.length : 0} shown`;
      const totalPages = Math.max(1, Math.ceil(this._total / this.perPage));
      const info = root.querySelector('#ev-page-info');
      if (info) info.textContent = `Page ${this.currentPage} of ${totalPages} (${this._total} total)`;
      const prev = root.querySelector('#ev-prev');
      const next = root.querySelector('#ev-next');
      if (prev) prev.disabled = this.currentPage <= 1;
      if (next) next.disabled = this.currentPage >= totalPages;
    },

    _key(e) {
      return `${e.timestamp}|${e.event_type}|${e.message}`;
    },

    _rowHtml(e, isNew) {
      const sev = String(e.severity || 'info').toLowerCase();
      const variant = SEV_VARIANT[sev] || 'neutral';
      const bar = SEV_VARIANT[sev]
        ? `<span class="severity-bar severity-bar--${sev}"></span>`
        : '';
      return `
        <tr${isNew ? ' class="event-row--new"' : ''}>
          <td class="mono">${BridgeUI.esc(BridgeUI.fmt.time(e.timestamp))} <span class="muted">${BridgeUI.esc(BridgeUI.fmt.ago(e.timestamp))}</span></td>
          <td>${bar}${BridgeUI.badge(sev, variant)}</td>
          <td class="mono">${BridgeUI.esc(e.event_type || '—')}</td>
          <td class="mono">${BridgeUI.esc(e.node_id || '—')}</td>
          <td class="cell-primary">${BridgeUI.esc(e.message || '—')}</td>
        </tr>`;
    },
  };
})();
