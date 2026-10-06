// BridgeAPI — centralized daemon API access. Every method resolves to
// { data, error } and never rejects; errors use the daemon's structured
// shape { error: "message", code: "ERROR_CODE" }.

// escapeHtml — sanitize API strings before interpolating into innerHTML
function escapeHtml(str) {
  return String(str).replace(/[&<>"']/g, c => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;'
  }[c]));
}

const BridgeAPI = {
  async _request(method, path, body, rawText) {
    try {
      const opts = { method, headers: {} };
      if (body !== undefined) {
        opts.headers['Content-Type'] = rawText ? 'text/plain' : 'application/json';
        opts.body = rawText ? body : JSON.stringify(body);
      }
      const res = await fetch(path, opts);
      const data = await res.json().catch(() => null);
      if (res.status === 401) {
        // Session expired or missing — the auth middleware rejects API calls.
        window.location.href = '/login';
        return { data: null, error: data || { error: 'authentication required', code: 'UNAUTHORIZED' }, status: 401 };
      }
      if (!res.ok) {
        return { data: null, error: data || { error: res.statusText, code: 'UNKNOWN' }, status: res.status };
      }
      return { data, error: null, status: res.status };
    } catch (e) {
      return { data: null, error: { error: e.message, code: 'NETWORK_ERROR' }, status: 0 };
    }
  },

  get(path)            { return this._request('GET', path); },
  post(path, body)     { return this._request('POST', path, body); },
  put(path, body)      { return this._request('PUT', path, body); },
  putText(path, text)  { return this._request('PUT', path, text, true); },
  del(path)            { return this._request('DELETE', path); },

  getStatus()          { return this.get('/api/v1/status'); },
  getNodes()           { return this.get('/api/v1/nodes'); },
  getNode(id)          { return this.get(`/api/v1/nodes/${encodeURIComponent(id)}`); },
  getRoutes()          { return this.get('/api/v1/routes'); },
  getRoute(hostname)   { return this.get(`/api/v1/routes/${encodeURIComponent(hostname)}`); },
  getMesh()            { return this.get('/api/v1/mesh'); },
  getMetrics()         { return this.get('/api/v1/metrics'); },
  getEvents(page = 1, perPage = 50, type = null) {
    let url = `/api/v1/events?page=${page}&per_page=${perPage}`;
    if (type) url += `&type=${encodeURIComponent(type)}`;
    return this.get(url);
  },
  getHandoff()         { return this.get('/api/v1/handoff'); },
  getConfig()          { return this.get('/api/v1/config'); },
  putConfig(doc)       { return this.put('/api/v1/config', doc); },
  putConfigToml(text)  { return this.putText('/api/v1/config', text); },
  getLogs(limit = 200, level = null) {
    let url = `/api/v1/logs?limit=${limit}`;
    if (level) url += `&level=${encodeURIComponent(level)}`;
    return this.get(url);
  },
  getHealth()          { return this.get('/health'); },
  getAuth()            { return this.get('/api/v1/auth'); },
  login(username, password) { return this.post('/api/v1/login', { username, password }); },
  logout()             { return this.post('/api/v1/logout', {}); },
  replicate(nodeId)    { return this.post('/api/v1/replicate', { node_id: nodeId }); },
  failback(domain)     { return this.post('/api/v1/failback', { domain }); },
};

// BridgeStream — WebSocket event stream with exponential-backoff reconnect.
class BridgeStream {
  constructor() {
    this._ws = null;
    this._listeners = [];
    this._reconnectDelay = 1000;
    this._maxDelay = 16000;
    this._connected = false;
  }
  connect() {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    this._ws = new WebSocket(`${proto}//${location.host}/api/v1/stream`);
    this._ws.onopen = () => {
      this._connected = true;
      this._reconnectDelay = 1000;
      document.dispatchEvent(new CustomEvent('bridge:ws', { detail: { connected: true } }));
    };
    this._ws.onmessage = (e) => {
      let event;
      try {
        event = JSON.parse(e.data);
      } catch (_) {
        return; // malformed frame — ignore, stream stays alive
      }
      this._listeners.forEach(fn => {
        try { fn(event); } catch (_) { /* a broken view must not kill the stream */ }
      });
    };
    this._ws.onclose = () => {
      this._connected = false;
      document.dispatchEvent(new CustomEvent('bridge:ws', { detail: { connected: false } }));
      setTimeout(() => this.connect(), this._reconnectDelay);
      this._reconnectDelay = Math.min(this._reconnectDelay * 2, this._maxDelay);
    };
    this._ws.onerror = () => this._ws.close();
  }
  onEvent(fn) { this._listeners.push(fn); }
  isConnected() { return this._connected; }
}
