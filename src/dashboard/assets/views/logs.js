// Logs view — daemon log buffer (GET /api/v1/logs, chronological oldest-first,
// rendered top-to-bottom so the newest line sits at the bottom). Follow mode
// refetches on every poll tick and pins the stream to the bottom.
'use strict';
(function () {
  const LEVELS = ['ALL', 'ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'];
  const KNOWN_LEVELS = ['ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'];
  const LIMITS = [100, 200, 500];

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.logs = {
    title: 'Logs',

    // Initialized once (not in render) so they persist across view re-entry.
    level: 'ALL',
    limit: 200,
    follow: true,

    _root: null,
    _loadedOnce: false,
    _inflight: false,

    async render(container, state) {
      const root = (this._root = container);
      container.innerHTML = BridgeUI.panel({
        title: 'Daemon Logs',
        actions: this._controlsHtml(),
        flush: true,
        body: '<div class="log-stream" id="log-stream" style="max-height:62vh;overflow-y:auto"></div>',
      });
      this._wire(state, root);
      if (!this._loadedOnce) {
        const stream = root.querySelector('#log-stream');
        if (stream) stream.innerHTML = BridgeUI.skeletons(3, 40);
      }
      await this._fetch(root);
    },

    update() {
      // Follow mode: refetch every poll tick. When follow is off, never touch
      // the DOM — the operator may be reading/scrolling history.
      if (!this.follow || this._inflight) return;
      const root = this._root;
      if (!root || !root.isConnected) return;
      this._fetch(root);
    },

    destroy() {
      this._root = null;
    },

    /* ------------------------------------------------------------ internals */

    _controlsHtml() {
      const levelButtons = LEVELS.map(l =>
        `<button class="filter-btn${this.level === l ? ' active' : ''}" data-level="${l}" type="button">${l === 'ALL' ? 'All' : l}</button>`
      ).join('');
      const limitOptions = LIMITS.map(n =>
        `<option value="${n}"${this.limit === n ? ' selected' : ''}>${n}</option>`
      ).join('');
      return `
        <div class="filter-bar" id="log-controls">
          ${levelButtons}
          <select class="field-select" id="log-limit" style="width:auto">${limitOptions}</select>
          <label class="field-checkbox" style="padding-top:0">
            <input type="checkbox" id="log-follow"${this.follow ? ' checked' : ''}>
            <span>Follow</span>
          </label>
        </div>`;
    },

    _wire(state, root) {
      const bar = root.querySelector('#log-controls');
      if (!bar) return;
      bar.addEventListener('click', (e) => {
        const btn = e.target.closest('[data-level]');
        if (!btn) return;
        this.level = btn.dataset.level;
        bar.querySelectorAll('[data-level]').forEach(b =>
          b.classList.toggle('active', b === btn));
        this._fetch(root);
      });
      const limitSel = bar.querySelector('#log-limit');
      if (limitSel) limitSel.addEventListener('change', () => {
        const v = parseInt(limitSel.value, 10);
        if (Number.isFinite(v)) this.limit = v;
        this._fetch(root);
      });
      const followCb = bar.querySelector('#log-follow');
      if (followCb) followCb.addEventListener('change', () => {
        this.follow = followCb.checked;
        if (this.follow) this._fetch(root); // catch up + re-pin to bottom
      });
    },

    async _fetch(root) {
      if (this._inflight) return;
      this._inflight = true;
      const res = await BridgeAPI.getLogs(this.limit, this.level === 'ALL' ? null : this.level);
      this._inflight = false;
      if (!root.isConnected || this._root !== root) return;

      const stream = root.querySelector('#log-stream');
      if (!stream) return;
      if (res.error) {
        // First-load failure gets the error state; after that, keep showing
        // the last good buffer rather than blanking it on a transient error.
        if (!this._loadedOnce) {
          stream.innerHTML = BridgeUI.errorState('Logs unavailable', res.error.error || 'Logs request failed');
        }
        return;
      }
      this._loadedOnce = true;
      const logs = (res.data && res.data.logs) || [];
      if (logs.length === 0) {
        stream.innerHTML = BridgeUI.emptyState(
          'No logs captured yet', 'The buffer fills as the daemon logs.');
        return;
      }
      stream.innerHTML = logs.map(l => this._lineHtml(l)).join('');
      if (this.follow) stream.scrollTop = stream.scrollHeight;
    },

    _lineHtml(l) {
      const lvl = String(l.level || 'INFO').toUpperCase();
      const cls = KNOWN_LEVELS.includes(lvl) ? lvl : 'INFO';
      const target = l.target
        ? `<span class="log-target">${BridgeUI.esc(l.target)}</span> `
        : '';
      return `
        <div class="log-line">
          <span class="log-time">${BridgeUI.esc(BridgeUI.fmt.time(l.timestamp))}</span>
          <span class="log-level log-level--${cls}">${BridgeUI.esc(lvl)}</span>
          <span class="log-message">${target}${BridgeUI.esc(l.message || '')}</span>
        </div>`;
    },
  };
})();
