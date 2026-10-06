#!/bin/sh
# BRIDGE installer — installs the bridge daemon from GitHub releases.
#
#   curl -fsSL https://bridgemesh.space/install.sh | sh
#
# Configuration via environment:
#   BRIDGE_REPO     GitHub repository        (default: Penivera/Bridge)
#   BRIDGE_VERSION  Release tag or "latest"  (default: latest)
#   INSTALL_DIR     Binary install directory (default: /usr/local/bin)
#
# POSIX sh; requires only standard utilities plus curl and tar.

set -eu

BRIDGE_REPO="${BRIDGE_REPO:-Penivera/Bridge}"
BRIDGE_VERSION="${BRIDGE_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

info() {
    printf 'info: %s\n' "$*"
}

err() {
    printf 'error: %s\n' "$*" >&2
}

die() {
    err "$1"
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

is_system_path() {
    case "$1" in
        /usr|/usr/*|/bin|/bin/*|/sbin|/sbin/*|/etc|/etc/*|/opt|/opt/*) return 0 ;;
        *) return 1 ;;
    esac
}

# --- platform detection -----------------------------------------------------

need_cmd uname
OS_NAME="$(uname -s)"
ARCH_NAME="$(uname -m)"

case "$OS_NAME" in
    Linux)  OS=linux ;;
    Darwin) OS=darwin ;;
    *)      die "unsupported operating system: $OS_NAME (supported: Linux, macOS)" ;;
esac

case "$OS/$ARCH_NAME" in
    linux/x86_64)            TARGET=x86_64-unknown-linux-musl ;;
    linux/aarch64|linux/arm64) TARGET=aarch64-unknown-linux-musl ;;
    *) die "unsupported architecture: $OS_NAME/$ARCH_NAME (supported: linux x86_64/aarch64)" ;;
esac

# --- privilege handling -----------------------------------------------------

if [ "$(id -u)" -ne 0 ] && is_system_path "$INSTALL_DIR"; then
    if command -v sudo >/dev/null 2>&1; then
        if [ -f "$0" ]; then
            info "installing to $INSTALL_DIR requires root; re-running via sudo..."
            exec sudo -E sh "$0" "$@"
        fi
        die "installing to $INSTALL_DIR requires root, but this script is running from a pipe. Re-run as: curl -fsSL https://bridgemesh.space/install.sh | sudo -E sh"
    fi
    die "root privileges are required to install to $INSTALL_DIR and sudo was not found. Re-run as root or set INSTALL_DIR to a user-writable directory."
fi

if [ "$(id -u)" -eq 0 ]; then
    IS_ROOT=1
else
    IS_ROOT=0
fi

need_cmd curl
need_cmd tar

# --- resolve version ----------------------------------------------------------

if [ "$BRIDGE_VERSION" = "latest" ]; then
    API_URL="https://api.github.com/repos/$BRIDGE_REPO/releases/latest"
    info "resolving latest release from $API_URL"
    RESPONSE="$(curl -fsSL --retry 3 -H 'Accept: application/vnd.github+json' "$API_URL")" \
        || die "could not query the GitHub API at $API_URL — check your network or set BRIDGE_VERSION to a specific tag"
    BRIDGE_VERSION="$(printf '%s\n' "$RESPONSE" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
    [ -n "$BRIDGE_VERSION" ] || die "could not parse tag_name from the GitHub API response — set BRIDGE_VERSION to a specific tag"
fi

info "installing bridge $BRIDGE_VERSION ($TARGET)"

# --- download ---------------------------------------------------------------

TMPDIR_INSTALL="$(mktemp -d)" || die "could not create a temporary directory"
trap 'rm -rf "$TMPDIR_INSTALL"' EXIT

ASSET="bridge-$BRIDGE_VERSION-$TARGET.tar.gz"
BASE_URL="https://github.com/$BRIDGE_REPO/releases/download/$BRIDGE_VERSION"

info "downloading $BASE_URL/$ASSET"
curl -fsSL --retry 3 -o "$TMPDIR_INSTALL/$ASSET" "$BASE_URL/$ASSET" \
    || die "download failed: $BASE_URL/$ASSET — does this release exist and publish that asset?"

info "downloading $BASE_URL/sha256sums.txt"
curl -fsSL --retry 3 -o "$TMPDIR_INSTALL/sha256sums.txt" "$BASE_URL/sha256sums.txt" \
    || die "download failed: $BASE_URL/sha256sums.txt — refusing to install without checksums"

# --- verify checksum (fail closed) -------------------------------------------

EXPECTED="$(awk -v f="$ASSET" '{ n = $NF; sub(/^\*/, "", n); if (n == f) print $1 }' "$TMPDIR_INSTALL/sha256sums.txt")"
[ -n "$EXPECTED" ] || die "no checksum entry for $ASSET in sha256sums.txt — refusing to install"

if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMPDIR_INSTALL/$ASSET" | awk '{ print $1 }')"
elif command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "$TMPDIR_INSTALL/$ASSET" | awk '{ print $1 }')"
else
    die "neither sha256sum nor shasum is available — cannot verify the download, refusing to install"
fi

if [ "$EXPECTED" != "$ACTUAL" ]; then
    die "checksum mismatch for $ASSET (expected $EXPECTED, got $ACTUAL) — refusing to install"
fi
info "checksum verified"

# --- extract and install ------------------------------------------------------

mkdir -p "$TMPDIR_INSTALL/extract"
tar -xzf "$TMPDIR_INSTALL/$ASSET" -C "$TMPDIR_INSTALL/extract" \
    || die "could not extract $ASSET"

BRIDGE_BIN="$(find "$TMPDIR_INSTALL/extract" -type f -name bridge | head -n 1)"
[ -n "$BRIDGE_BIN" ] || die "the archive did not contain a 'bridge' binary"

mkdir -p "$INSTALL_DIR" || die "could not create install directory: $INSTALL_DIR"
TMP_TARGET="$INSTALL_DIR/.bridge.tmp.$$"
cp "$BRIDGE_BIN" "$TMP_TARGET" || die "could not copy the binary into $INSTALL_DIR"
chmod 0755 "$TMP_TARGET" || die "could not set permissions on $TMP_TARGET"
mv -f "$TMP_TARGET" "$INSTALL_DIR/bridge" || die "could not install $INSTALL_DIR/bridge"
info "installed $INSTALL_DIR/bridge"

if "$INSTALL_DIR/bridge" --version >/dev/null 2>&1; then
    info "bridge --version: $("$INSTALL_DIR/bridge" --version 2>/dev/null)"
else
    err "warning: '$INSTALL_DIR/bridge --version' did not run cleanly; the binary is installed but please check it manually"
fi

# --- system setup (Linux + root only) ----------------------------------------

SERVICE_STATE="not configured"

if [ "$OS" = linux ] && [ "$IS_ROOT" -eq 1 ]; then
    if ! id bridge >/dev/null 2>&1; then
        useradd --system --no-create-home --shell /usr/sbin/nologin bridge \
            || die "could not create the 'bridge' system user"
        info "created 'bridge' system user"
    fi

    mkdir -p /etc/bridge || die "could not create /etc/bridge"
    chown root:bridge /etc/bridge || die "could not set ownership on /etc/bridge"
    chmod 0750 /etc/bridge || die "could not set permissions on /etc/bridge"

    if [ ! -f /etc/bridge/bridge.toml ]; then
        cat > /etc/bridge/bridge.toml <<'EOF'
enable_telemetry = false

[proxy]
mode = "Direct"
listeners = ["http"]
http_addr = "0.0.0.0:80"

[dashboard]
enabled = true
listen_addr = "127.0.0.1:9090"

[logger]
level = "INFO"
format = "text"
EOF
        chown root:bridge /etc/bridge/bridge.toml || die "could not set ownership on /etc/bridge/bridge.toml"
        chmod 0640 /etc/bridge/bridge.toml || die "could not set permissions on /etc/bridge/bridge.toml"
        info "wrote minimal standalone config to /etc/bridge/bridge.toml"
    else
        info "existing /etc/bridge/bridge.toml found — leaving it untouched"
    fi

    if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
        cat > /etc/systemd/system/bridge.service <<EOF
[Unit]
Description=BRIDGE daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=bridge
Group=bridge
ExecStart=$INSTALL_DIR/bridge --config /etc/bridge/bridge.toml
Restart=on-failure
RestartSec=3
AmbientCapabilities=CAP_NET_BIND_SERVICE CAP_NET_ADMIN
LimitNOFILE=65535
ProtectSystem=full
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF
        info "wrote /etc/systemd/system/bridge.service"
        systemctl daemon-reload || die "systemctl daemon-reload failed"
        systemctl enable --now bridge \
            || die "the bridge service failed to start — inspect with: journalctl -u bridge -n 50"
        SERVICE_STATE="enabled and running (systemd)"
    else
        info "systemd not detected — run the daemon manually:"
        info "  $INSTALL_DIR/bridge --config /etc/bridge/bridge.toml"
        SERVICE_STATE="not started (no systemd)"
    fi
elif [ "$OS" = linux ]; then
    info "non-root install: skipping system user, /etc/bridge, and service setup"
    info "run the daemon manually: $INSTALL_DIR/bridge --config /path/to/bridge.toml"
else
    info "service setup is Linux-only — run the daemon manually:"
    info "  $INSTALL_DIR/bridge --config /path/to/bridge.toml"
fi

# --- summary ------------------------------------------------------------------

printf '\n'
info "BRIDGE $BRIDGE_VERSION installed"
printf '  binary:    %s\n' "$INSTALL_DIR/bridge"
if [ "$OS" = linux ] && [ "$IS_ROOT" -eq 1 ]; then
    printf '  config:    %s\n' "/etc/bridge/bridge.toml"
    printf '  service:   %s\n' "$SERVICE_STATE"
    printf '  status:    %s\n' "systemctl status bridge"
else
    printf '  config:    %s\n' "create bridge.toml and pass --config (see docs)"
fi
printf '  dashboard: %s\n' "http://127.0.0.1:9090"
printf '  docs:      %s\n' "https://bridgemesh.space/docs/getting-started.html"
