// Runtime view (file: config.js, route: runtime): configuration editor
// backed by GET + PUT /api/v1/config.
//
// Preservation contract: the form only covers well-known operational fields.
// On save, form values are MERGED into a deep copy of the pristine document,
// so unmanaged sections (nodes, seeds, services, sentry, handoff.tunnel/dns,
// udp_services, unknown keys) survive untouched. Raw mode PUTs the edited
// document verbatim.
'use strict';
(function () {
  const SECTIONS = [
    { title: 'General', fields: [
      { path: 'enable_telemetry', label: 'Enable telemetry', type: 'checkbox' },
    ]},
    { title: 'Proxy', fields: [
      { path: 'proxy.mode', label: 'Mode', type: 'select',
        options: [['Direct', 'Direct'], ['SniPassthrough', 'SniPassthrough'], ['Managed', 'Managed']] },
      { path: 'proxy.listeners', label: 'Listeners', type: 'list', mono: true,
        hint: 'Comma-separated, e.g. http, https' },
      { path: 'proxy.http_addr', label: 'HTTP address', type: 'text', mono: true },
      { path: 'proxy.https_addr', label: 'HTTPS address', type: 'text', mono: true },
      { path: 'proxy.redirect_http', label: 'Redirect HTTP to HTTPS', type: 'checkbox' },
    ]},
    { title: 'Dashboard', fields: [
      { path: 'dashboard.enabled', label: 'Enabled', type: 'checkbox' },
      { path: 'dashboard.listen_addr', label: 'Listen address', type: 'text', mono: true },
    ]},
    { title: 'Discovery', fields: [
      { path: 'discovery.enabled', label: 'Enabled', type: 'checkbox' },
      { path: 'discovery.docker_socket', label: 'Docker socket', type: 'text', mono: true },
      { path: 'discovery.default_node_id', label: 'Default node ID', type: 'text', mono: true },
    ]},
    { title: 'IPC', fields: [
      { path: 'ipc.enabled', label: 'Enabled', type: 'checkbox' },
      { path: 'ipc.socket_path', label: 'Socket path', type: 'text', mono: true },
    ]},
    { title: 'Handoff', fields: [
      { path: 'handoff.mode', label: 'Mode', type: 'select',
        options: [['none', 'none'], ['dns', 'dns'], ['tunnel', 'tunnel'], ['floating_ip', 'floating_ip']] },
    ]},
    { title: 'Logging', fields: [
      { path: 'logger.level', label: 'Level', type: 'select',
        options: [['ERROR', 'ERROR'], ['WARN', 'WARN'], ['INFO', 'INFO'], ['DEBUG', 'DEBUG'], ['TRACE', 'TRACE']] },
      { path: 'logger.format', label: 'Format', type: 'select',
        options: [['text', 'text'], ['json', 'json']] },
      { path: 'logger.ansi', label: 'ANSI colors', type: 'checkbox' },
    ]},
  ];
  const ALL_FIELDS = SECTIONS.flatMap(s => s.fields);

  window.BridgeViews = window.BridgeViews || {};
  window.BridgeViews.runtime = {
    title: 'Runtime',

    _root: null,
    _pristine: null,   // deep copy of the last loaded/saved config document
    _dirty: false,
    _rawMode: false,
    _rawBase: null,    // doc captured when entering raw mode (form edits carried over)
    _noFile: false,    // set after a 409: saving is impossible without a config file

    async render(container) {
      const root = (this._root = container);
      this._rawMode = false;
      this._rawBase = null;
      this._dirty = false;
      this._noFile = false;
      container.innerHTML = BridgeUI.skeletons(2, 220);

      const res = await BridgeAPI.getConfig();
      if (!root.isConnected || this._root !== root) return;

      if (res.error) {
        container.innerHTML = BridgeUI.errorState(
          'Configuration unavailable', res.error.error || 'Request failed');
        return;
      }
      const cfg = res.data && res.data.config && typeof res.data.config === 'object'
        ? res.data.config
        : {};
      this._pristine = this._deepCopy(cfg);
      this._renderShell(Object.keys(cfg).length === 0);
    },

    update() {
      // Intentionally a no-op: /api/v1/config is not part of the poll cycle,
      // and re-rendering on a tick could clobber a dirty form. The dirty
      // guard (this._dirty) is the contract: nothing here may touch the DOM.
    },

    destroy() {
      this._root = null;
    },

    /* ------------------------------------------------------------ shell */

    _renderShell(isDefaults) {
      const root = this._root;
      root.innerHTML = `
        ${isDefaults ? `<div class="form-banner form-banner--info">No configuration loaded: the daemon is running on built-in defaults. Saving changes will fail (409) until the daemon is started with a config file.</div>` : ''}
        <div id="cfg-banner"></div>
        <section class="panel">
          <div class="panel-header">
            <span class="panel-title">Configuration</span>
            <div class="panel-actions">
              <button class="btn btn--sm" id="cfg-advanced" type="button">Advanced: edit raw document</button>
            </div>
          </div>
          <div class="panel-body" id="cfg-body"></div>
        </section>
        ${BridgeUI.panel({
          title: 'Nodes',
          actions: '<span class="muted small">Managed via the daemon config file or advanced editor below.</span>',
          body: '<div id="cfg-nodes-body"></div>',
        })}
        ${BridgeUI.panel({
          title: 'Services',
          actions: '<span class="muted small">Managed via the daemon config file or advanced editor below.</span>',
          body: '<div id="cfg-services-body"></div>',
        })}`;

      const body = root.querySelector('#cfg-body');
      body.innerHTML = this._formBodyHtml();
      this._renderReadonlyPanels();

      // #cfg-body is stable across mode swaps (only its innerHTML changes), so
      // one delegated listener set handles form input + Save/Cancel in both modes.
      body.addEventListener('input', () => this._refreshDirty());
      body.addEventListener('change', () => this._refreshDirty());
      body.addEventListener('click', (e) => {
        if (e.target.closest('#cfg-save')) this._save();
        else if (e.target.closest('#cfg-cancel')) this._cancel();
      });
      root.querySelector('#cfg-advanced').addEventListener('click', () => this._toggleRaw());
      this._refreshDirty();
    },

    _actionsHtml() {
      return `
        <div class="form-actions">
          <button class="btn btn--primary" id="cfg-save" type="button" disabled>Save</button>
          <button class="btn" id="cfg-cancel" type="button">Cancel</button>
          <span class="unsaved-indicator" id="cfg-unsaved" style="display:none">Unsaved changes</span>
        </div>`;
    },

    _formBodyHtml(valuesDoc) {
      const doc = valuesDoc || this._pristine || {};
      const sections = SECTIONS.map(sec => `
        <div class="form-section">
          <div class="form-section-title">${BridgeUI.esc(sec.title)}</div>
          <div class="form-grid">${sec.fields.map(f => this._fieldHtml(f, doc)).join('')}</div>
        </div>`).join('');
      return sections + this._actionsHtml();
    },

    _fieldHtml(f, doc) {
      const raw = this._getPath(doc, f.path);
      if (f.type === 'checkbox') {
        return BridgeUI.field({
          label: f.label, name: f.path, type: 'checkbox', value: raw === true, hint: f.hint || '',
        });
      }
      if (f.type === 'select') {
        const options = f.options.map(([value, label]) => ({ value, label }));
        const current = raw === undefined || raw === null ? null : String(raw);
        // A loaded value outside the known list (e.g. a mode from a newer
        // daemon) stays selectable so it is never silently rewritten.
        if (current !== null && !options.some(o => o.value === current)) {
          options.push({ value: current, label: current });
        }
        return BridgeUI.field({
          label: f.label, name: f.path, type: 'select',
          value: current === null ? f.options[0][0] : current, options, hint: f.hint || '',
        });
      }
      if (f.type === 'list') {
        const text = Array.isArray(raw) ? raw.join(', ') : (raw === undefined || raw === null ? '' : String(raw));
        return BridgeUI.field({
          label: f.label, name: f.path, type: 'text', value: text, mono: Boolean(f.mono), hint: f.hint || '',
        });
      }
      const text = raw === undefined || raw === null
        ? ''
        : (typeof raw === 'object' ? JSON.stringify(raw) : String(raw));
      return BridgeUI.field({
        label: f.label, name: f.path, type: 'text', value: text, mono: Boolean(f.mono), hint: f.hint || '',
      });
    },

    _rawBodyHtml(doc) {
      const text = JSON.stringify(doc || this._pristine || {}, null, 2);
      return `
        <div class="form-section">
          ${BridgeUI.field({
            label: 'Raw configuration document (JSON)',
            name: 'cfg-raw',
            type: 'textarea',
            value: text,
            mono: true,
            hint: 'The full document is sent verbatim on Save: keep any sections you want to preserve.',
          })}
        </div>` + this._actionsHtml();
    },

    _renderReadonlyPanels() {
      const cfg = this._pristine || {};
      const nodesBody = this._root && this._root.querySelector('#cfg-nodes-body');
      if (nodesBody) {
        const nodes = Array.isArray(cfg.nodes) ? cfg.nodes : [];
        nodesBody.innerHTML = BridgeUI.table({
          columns: [
            { label: 'Node', mono: true, primary: true, render: n => BridgeUI.esc(this._nodeId(n)) },
            { label: 'Endpoint', mono: true, render: n => BridgeUI.esc(this._nodeEndpoint(n)) },
          ],
          rows: nodes,
          empty: 'No static nodes configured',
        });
      }
      const servicesBody = this._root && this._root.querySelector('#cfg-services-body');
      if (servicesBody) {
        const services = Array.isArray(cfg.services) ? cfg.services : [];
        servicesBody.innerHTML = BridgeUI.table({
          columns: [
            { label: 'URL', mono: true, primary: true, render: s => BridgeUI.esc(this._svcField(s, ['url', 'domain', 'host'])) },
            { label: 'Node', mono: true, render: s => BridgeUI.esc(this._svcField(s, ['node_id', 'node'])) },
            { label: 'Upstream', mono: true, render: s => BridgeUI.esc(this._svcField(s, ['upstream', 'upstream_addr', 'target'])) },
          ],
          rows: services,
          empty: 'No services configured',
        });
      }
    },

    _nodeId(n) {
      if (typeof n === 'string') return n;
      return (n && (n.node_id || n.id || n.name)) || '–';
    },

    _nodeEndpoint(n) {
      if (!n || typeof n !== 'object') return '–';
      return n.endpoint || n.address || n.mesh_address || '–';
    },

    _svcField(s, keys) {
      if (!s || typeof s !== 'object') return '–';
      for (const k of keys) {
        if (s[k] !== undefined && s[k] !== null && s[k] !== '') return String(s[k]);
      }
      return '–';
    },

    /* ------------------------------------------------------------ dirty */

    _serializeForm() {
      const doc = this._deepCopy(this._pristine || {});
      const body = this._root && this._root.querySelector('#cfg-body');
      if (!body) return doc;
      for (const f of ALL_FIELDS) {
        const el = body.querySelector(`[name="${f.path}"]`);
        if (!el) continue;
        let value;
        if (f.type === 'checkbox') value = el.checked;
        else if (f.type === 'list') value = el.value.split(',').map(s => s.trim()).filter(Boolean);
        else value = el.value;
        if (this._hasPath(this._pristine, f.path)) {
          this._setPath(doc, f.path, value);
          continue;
        }
        // Absent in the pristine doc: only write the value if the operator
        // actually changed it from the rendered default: otherwise an
        // untouched form would appear dirty (and grow phantom keys on save).
        const initial = f.type === 'checkbox' ? false
          : f.type === 'list' ? []
          : f.type === 'select' ? f.options[0][0]
          : '';
        const changed = f.type === 'list'
          ? JSON.stringify(value) !== JSON.stringify(initial)
          : value !== initial;
        if (changed) this._setPath(doc, f.path, value);
      }
      return doc;
    },

    _refreshDirty() {
      const root = this._root;
      if (!root) return;
      const save = root.querySelector('#cfg-save');
      const indicator = root.querySelector('#cfg-unsaved');
      if (!save) return;
      let dirty = false;
      if (this._rawMode) {
        const ta = root.querySelector('[name="cfg-raw"]');
        dirty = ta ? ta.value !== JSON.stringify(this._pristine || {}, null, 2) : false;
      } else {
        dirty = JSON.stringify(this._serializeForm()) !== JSON.stringify(this._pristine || {});
      }
      this._dirty = dirty;
      if (indicator) indicator.style.display = dirty ? '' : 'none';
      save.disabled = !dirty || this._noFile;
    },

    /* ------------------------------------------------------------ actions */

    async _save() {
      const root = this._root;
      const body = root && root.querySelector('#cfg-body');
      const save = root && root.querySelector('#cfg-save');
      if (!body || !save) return;

      let doc;
      if (this._rawMode) {
        const ta = body.querySelector('[name="cfg-raw"]');
        try {
          doc = JSON.parse(ta.value);
        } catch (e) {
          this._setBanner('error', `Invalid JSON: ${e.message}`);
          return;
        }
        if (!doc || typeof doc !== 'object' || Array.isArray(doc)) {
          this._setBanner('error', 'Configuration document must be a JSON object.');
          return;
        }
      } else {
        doc = this._serializeForm();
      }

      this._clearBanner();
      this._clearFieldErrors();
      BridgeUI.setLoading(save, true);
      const res = await BridgeAPI.putConfig(doc);
      if (!root.isConnected || this._root !== root) return;
      BridgeUI.setLoading(save, false);

      if (!res.error) {
        this._pristine = this._deepCopy(res.data && res.data.config ? res.data.config : doc);
        this._noFile = false;
        this._rawBase = null;
        const note = String((res.data && res.data.message) || '')
          .replace(/^configuration saved\.?\s*/i, '');
        this._setBanner('success', `Configuration saved.${note ? ` ${note}` : ''}`);
        BridgeUI.toast('Configuration saved', 'success');
        // Rebuild from the server-echoed document so normalized values show.
        body.innerHTML = this._rawMode ? this._rawBodyHtml() : this._formBodyHtml();
        this._fixupBody();
        this._renderReadonlyPanels();
        this._dirty = false;
        this._refreshDirty();
        return;
      }

      const errText = res.error.error || 'Save failed';
      const code = res.error.code || '';
      if (res.status === 409 || code === 'CONFLICT') {
        this._setBanner('error', 'Running on defaults: no config file loaded, changes cannot be persisted.');
        this._noFile = true;
        save.disabled = true;
        return;
      }
      if (res.status === 422 || code === 'VALIDATION_ERROR') {
        this._setBanner('error', errText);
        if (!this._rawMode) {
          for (const f of ALL_FIELDS) {
            if (errText.includes(f.path)) {
              const fieldEl = root.querySelector(`[data-field="${f.path}"]`);
              if (fieldEl) BridgeUI.setFieldError(fieldEl, errText);
            }
          }
        }
        return;
      }
      this._setBanner('error', errText);
    },

    _cancel() {
      const root = this._root;
      const body = root && root.querySelector('#cfg-body');
      if (!body) return;
      this._clearBanner();
      this._clearFieldErrors();
      // Restore from pristine: discards form edits and (in raw mode) edits
      // made to the textarea since entering raw mode.
      body.innerHTML = this._rawMode ? this._rawBodyHtml() : this._formBodyHtml();
      this._fixupBody();
      this._dirty = false;
      this._refreshDirty();
    },

    _toggleRaw() {
      const root = this._root;
      const body = root && root.querySelector('#cfg-body');
      if (!body) return;
      this._clearBanner();
      this._clearFieldErrors();
      if (!this._rawMode) {
        this._rawBase = this._serializeForm(); // carry current form edits into raw
        this._rawMode = true;
        body.innerHTML = this._rawBodyHtml(this._rawBase);
      } else {
        this._rawMode = false;
        // Back to the form: textarea edits are only applied via Save, so the
        // form rebuilds from the doc captured when raw mode was entered.
        body.innerHTML = this._formBodyHtml(this._rawBase || this._pristine);
      }
      this._fixupBody();
      const adv = root.querySelector('#cfg-advanced');
      if (adv) adv.textContent = this._rawMode ? 'Back to form' : 'Advanced: edit raw document';
      this._refreshDirty();
    },

    _fixupBody() {
      const ta = this._root && this._root.querySelector('[name="cfg-raw"]');
      if (ta) ta.rows = 24;
    },

    _setBanner(kind, text) {
      const el = this._root && this._root.querySelector('#cfg-banner');
      if (el) el.innerHTML = `<div class="form-banner form-banner--${kind}">${BridgeUI.esc(text)}</div>`;
    },

    _clearBanner() {
      const el = this._root && this._root.querySelector('#cfg-banner');
      if (el) el.innerHTML = '';
    },

    _clearFieldErrors() {
      const root = this._root;
      if (!root) return;
      root.querySelectorAll('.field.has-error').forEach(f => BridgeUI.setFieldError(f, ''));
    },

    /* ------------------------------------------------------------ path utils */

    _deepCopy(o) {
      return JSON.parse(JSON.stringify(o));
    },

    _hasPath(obj, path) {
      let o = obj;
      for (const p of path.split('.')) {
        if (!o || typeof o !== 'object' || !Object.prototype.hasOwnProperty.call(o, p)) return false;
        o = o[p];
      }
      return true;
    },

    _getPath(obj, path) {
      let o = obj;
      for (const p of path.split('.')) {
        if (!o || typeof o !== 'object') return undefined;
        o = o[p];
      }
      return o;
    },

    _setPath(obj, path, value) {
      const parts = path.split('.');
      let o = obj;
      for (let i = 0; i < parts.length - 1; i++) {
        if (!o[parts[i]] || typeof o[parts[i]] !== 'object') o[parts[i]] = {};
        o = o[parts[i]];
      }
      o[parts[parts.length - 1]] = value;
    },
  };
})();
