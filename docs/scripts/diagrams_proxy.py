"""Artifacts: Proxy Architecture & Routing Diagrams for BRIDGE.

09 — Proxy Modes: Direct (L7) vs Handoff (L4 SNI Passthrough)
10 — Sequence: SNI-Based Handoff Routing Flow
11 — Architectural Separation: Global Entry-Point vs Local Service Router
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import matplotlib.patches as mp
import matplotlib.pyplot as plt

from style import (arrow, box, box_, figure, footer, label, legend, PALETTE, save,
                   title, circle, WHITE, HAIRLINE)


# --- 09. Proxy Modes: Direct vs Handoff Architecture -----------------------
def fig09_proxy_modes(out):
    fig, ax = figure("Proxy Modes", 11.0, 7.6)
    title(fig, "Proxy Architecture: Direct vs Handoff Modes",
          "Direct mode terminates TLS & routes L7 HTTP; Handoff mode routes L4 TCP via SNI passthrough.",
          "09")

    # ================= LEFT COLUMN: DIRECT MODE (L7) =================
    box_(4, 12, 44, 73, fill=PALETTE["slate_lt"], edge=PALETTE["slate"], radius=1.4, lw=1.5)
    label(26, 82, "MODE 1: DIRECT (L7 Reverse Proxy)", fontsize=10.5, weight="bold", color=PALETTE["navy"])
    label(26, 79.2, "domain → Bridge (L7) → VM → service", fontsize=8.4, color=PALETTE["slate_dk"], style="italic")

    # Client Ingress
    box(14, 70, 24, 6.8, "Client (Browser / API)", fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
        fontsize=9.2, weight="bold", sub="HTTPS request to domain", sub_size=8.0)

    arrow((26, 70), (26, 62), color=PALETTE["navy"], lw=1.5)
    label(34.0, 66, "TLS Handshake", fontsize=8.6, color=PALETTE["navy"], weight="bold")

    # Bridge in Direct Mode
    box_(8, 43, 36, 19, fill=PALETTE["white"], edge=PALETTE["navy"], radius=1.0, lw=1.6)
    label(26, 59.5, "BRIDGE Leader (L7 Proxy)", fontsize=10.2, weight="bold", color=PALETTE["navy"])
    box(9.5, 45, 16.0, 10.5, "TLS Engine\n(Rustls)", fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
        fontsize=8.8, weight="bold", sub="terminates TLS", sub_size=8.0)
    box(26.5, 45, 16.0, 10.5, "HTTP Router\n(Hyper / L7)", fill=PALETTE["white"], edge=PALETTE["slate"],
        fontsize=8.8, weight="bold", sub="inspects Host/Path", sub_size=8.0)

    arrow((26, 43), (26, 34), color=PALETTE["slate_dk"], lw=1.5)
    label(36.5, 38.5, "WireGuard (HTTP/TCP)", fontsize=8.5, color=PALETTE["slate_dk"], weight="bold")

    # Backend VM & Service
    box_(8, 15, 36, 19, fill=PALETTE["white"], edge=PALETTE["teal"], radius=1.0, lw=1.4)
    label(26, 31.5, "Destination VM (e.g. VM-03)", fontsize=9.8, weight="bold", color=PALETTE["navy"])
    box(10.5, 17, 31, 10.5, "Target App Container / Service", fill=PALETTE["green_lt"],
        edge=PALETTE["green"], fontsize=9.0, weight="bold", sub="port: 3000 (direct connection)", sub_size=8.0)

    # ================= RIGHT COLUMN: HANDOFF MODE (L4 SNI) =================
    box_(52, 12, 44, 73, fill=PALETTE["teal_lt"], edge=PALETTE["teal"], radius=1.4, lw=1.8)
    label(74, 82, "MODE 2: HANDOFF (L4 SNI Passthrough) ★", fontsize=10.5, weight="bold", color=PALETTE["navy"])
    label(74, 79.2, "domain → Bridge (L4) → VM's Coolify Proxy → service", fontsize=8.4, color=PALETTE["slate_dk"], style="italic")

    # Client Ingress
    box(62, 70, 24, 6.8, "Client (Browser / API)", fill=PALETTE["white"], edge=PALETTE["teal"],
        fontsize=9.2, weight="bold", sub="ClientHello (SNI: app.example.com)", sub_size=8.0)

    arrow((74, 70), (74, 62), color=PALETTE["navy"], lw=1.5)
    label(83.5, 66, "Raw TLS Stream", fontsize=8.6, color=PALETTE["navy"], weight="bold")

    # Bridge in Handoff Mode
    box_(56, 43, 36, 19, fill=PALETTE["white"], edge=PALETTE["navy"], radius=1.0, lw=1.6)
    label(74, 59.5, "BRIDGE Leader (SNI L4 Router)", fontsize=10.2, weight="bold", color=PALETTE["navy"])
    box(57.5, 45, 16.0, 10.5, "SNI Parser\n(Peek ClientHello)", fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
        fontsize=8.8, weight="bold", sub="NO TLS termination", sub_size=8.0)
    box(74.5, 45, 16.0, 10.5, "Domain Registry\n(L4 Forwarder)", fill=PALETTE["white"], edge=PALETTE["navy"],
        fontsize=8.8, weight="bold", sub="app.example.com → VM-03", sub_size=8.0)

    arrow((74, 43), (74, 34), color=PALETTE["navy"], lw=1.6)
    label(84.5, 38.5, "WireGuard (Raw TLS/TCP)", fontsize=8.5, color=PALETTE["navy"], weight="bold")

    # Backend VM with Coolify Proxy
    box_(56, 15, 36, 19, fill=PALETTE["white"], edge=PALETTE["teal"], radius=1.0, lw=1.4)
    label(74, 31.5, "Destination VM (VM-03)", fontsize=9.8, weight="bold", color=PALETTE["navy"])
    box(57.5, 17, 16.0, 10.5, "Coolify Proxy\n(:443 Traefik/Caddy)", fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
        fontsize=8.8, weight="bold", sub="local SSL / ACME", sub_size=8.0)
    box(74.5, 17, 16.0, 10.5, "App Container\n(Docker)", fill=PALETTE["green_lt"], edge=PALETTE["green"],
        fontsize=8.8, weight="bold", sub="internal port", sub_size=8.0)

    arrow((73.5, 22.2), (74.5, 22.2), color=PALETTE["slate_dk"], lw=1.2)

    # Legend
    legend([
        (PALETTE["navy"], "BRIDGE L4/L7 Entry Router"),
        (PALETTE["teal"], "Coolify Local Proxy (Node L7)"),
        (PALETTE["green"], "Target App Service"),
        (PALETTE["slate"], "TCP / WireGuard Stream", "line"),
    ], x=2.5, y=3.2, w=27, h=12.5, title="Component Legend")

    footer(fig)
    return save(fig, out)


# --- 10. Sequence: SNI Handoff Routing Flow --------------------------------
def fig10_seq_sni_handoff(out):
    fig, ax = figure("SNI Handoff Sequence", 11.0, 7.6)
    title(fig, "Sequence: SNI-Based Handoff Routing (L4 Passthrough)",
          "Bridge inspects TLS ClientHello SNI → routes TCP stream to VM's Coolify proxy without TLS termination.",
          "10")

    actors = [("Client", 10), ("BRIDGE (L4)", 32),
              ("Domain Registry", 52), ("VM-03 (Coolify:443)", 74), ("App Container", 92)]

    # Scaffold lifelines
    for name, x in actors:
        ax.plot([x, x], [8, 82], color=PALETTE["hairline"], lw=1.0, zorder=1)
        bx = x - 7.0
        ax.add_patch(mp.FancyBboxPatch(
            (bx, 82), 14.0, 5.2, boxstyle="round,pad=0.02,rounding_size=0.8",
            linewidth=1.2, edgecolor=PALETTE["navy"],
            facecolor=PALETTE["navy"], zorder=3))
        ax.text(x, 82 + 2.6, name, ha="center", va="center", fontsize=9.0,
                weight="bold", color=PALETTE["white"], zorder=4)

    # Time markers helper - high contrast Navy
    def _timer(y, label):
        ax.text(1.5, y, label, fontsize=8.2, color=PALETTE["navy"], ha="left", va="center", weight="bold")

    def _msg(x1, x2, y, txt, *, dashed=False, color=None, fs=8.6):
        c = color or PALETTE["slate_dk"]
        style = "-|>"
        a = mp.FancyArrowPatch((x1, y), (x2, y), arrowstyle=style,
                               color=c, lw=1.3, linestyle="--" if dashed else "-",
                               mutation_scale=11, zorder=4)
        ax.add_patch(a)
        mx = (x1 + x2) / 2
        ax.text(mx, y + 1.2, txt, ha="center", va="bottom", fontsize=fs,
                color=PALETTE["slate_dk"], weight="bold", zorder=5)

    def _note(x, y, txt, *, w=25, h=3.4, color=None, fs=8.2):
        c = color or PALETTE["amber"]
        ax.add_patch(mp.FancyBboxPatch(
            (x - w/2, y), w, h, boxstyle="round,pad=0.02,rounding_size=0.5",
            linewidth=0.9, edgecolor=c, facecolor=PALETTE["white"], zorder=2))
        ax.text(x, y + h/2, txt, ha="center", va="center", fontsize=fs,
                color=PALETTE["slate_dk"], style="italic", zorder=3)

    def _act(x, y_top, y_bot, color=None):
        c = color or PALETTE["teal"]
        ax.add_patch(mp.Rectangle((x - 0.8, y_bot), 1.6, y_top - y_bot,
                                  facecolor=c, edgecolor=c, zorder=3, alpha=0.4))

    # 1. TCP Handshake + ClientHello
    y = 76
    _timer(y, "T₀ (Ingress)")
    _msg(10, 32, y, "TCP SYN / Handshake on :443")
    _act(32, y, 21, color=PALETTE["navy"])

    y = 69
    _timer(y, "T₀+2ms")
    _msg(10, 32, y, "TLS ClientHello (SNI: app.example.com)", color=PALETTE["navy"])
    _note(32, y - 5.0, "Peek ClientHello bytes (no TLS decrypt)", color=PALETTE["teal"], w=26, h=3.2)

    # 2. Domain Registry Lookup
    y = 59
    _timer(y, "T₀+4ms")
    _msg(32, 52, y, "LookupDomain('app.example.com')")
    _act(52, y, y - 5.5)
    _msg(52, 32, y - 5.5, "VM-03 (10.8.0.3:443)", dashed=True)
    _note(52, y - 10.5, "registry: domain → destination VM", color=PALETTE["amber"], w=25, h=3.2)

    # 3. Transparent TCP forwarding to VM-03 Coolify Proxy
    y = 42
    _timer(y, "T₀+6ms")
    _msg(32, 74, y, "WireGuard TCP Connect + forward ClientHello", color=PALETTE["navy"])
    _act(74, y, 13, color=PALETTE["teal"])
    _note(53, y - 5.0, "Zero-copy TCP splice / pipe (L4 Passthrough)", color=PALETTE["teal"], w=27, h=3.2)

    # 4. End-to-End TLS Handshake completed at Coolify Proxy
    y = 31
    _timer(y, "T₀+12ms")
    _msg(74, 10, y, "TLS ServerHello + Certificate (Let's Encrypt on VM-03)", color=PALETTE["teal"], dashed=True)

    # 5. Application Request & Local Container Routing
    y = 23
    _timer(y, "T₀+18ms")
    _msg(10, 74, y, "Encrypted HTTPS GET /api/v1 (via Bridge L4 pipe)")
    _msg(74, 92, y - 4.2, "Decrypt & reverse proxy to app container", color=PALETTE["green"])
    _act(92, y - 4.2, y - 8.4, color=PALETTE["green"])
    _msg(92, 74, y - 8.4, "HTTP 200 OK + payload", color=PALETTE["green"], dashed=True)
    _msg(74, 10, y - 12.2, "Encrypted HTTPS Response (via Bridge L4 pipe)", color=PALETTE["green"], dashed=True)

    footer(fig)
    return save(fig, out)


# --- 11. Architectural Separation: Global vs Local Router -------------------
def fig11_separation(out):
    fig, ax = figure("Architectural Separation", 11.0, 7.6)
    title(fig, "Architectural Separation: Global Entry-Point vs Local Service Router",
          "Bridge manages global entry & fleet L4 routing; local Coolify proxies manage container L7 routing.",
          "11")

    # Outer Global Boundary Box
    box_(4, 52, 92, 34, fill=PALETTE["teal_lt"], edge=PALETTE["teal"], radius=1.4, lw=1.8)
    label(50, 83.5, "GLOBAL ENTRY-POINT & FLEET TRAFFIC ROUTER (BRIDGE)", fontsize=11.0, weight="bold", color=PALETTE["navy"])
    label(50, 80.8, "Global public entry point · WireGuard L3 mesh · SWIM Gossip · Leader Election · SNI L4 Router",
          fontsize=8.4, color=PALETTE["slate_dk"], style="italic")

    # Global Components
    box(7, 55, 26, 22, "Global Entry Ingress\n(Cloudflare / Anycast)", fill=PALETTE["navy"], edge=PALETTE["navy"],
        tcolor=PALETTE["white"], fontsize=9.2, weight="bold", sub="Single public IP / DNS entry", sub_size=8.0, sub_color=WHITE)
    box(37, 55, 26, 22, "SNI L4 Router Engine\n(Tokio TCP Splice)", fill=PALETTE["white"], edge=PALETTE["navy"],
        fontsize=9.2, weight="bold", sub="Inspects ClientHello SNI\nZero-overhead L4 passthrough", sub_size=8.0)
    box(67, 55, 26, 22, "Domain Registry\n(SWIM Gossiped)", fill=PALETTE["white"], edge=PALETTE["teal"],
        fontsize=9.2, weight="bold", sub="app.example.com → VM-03\napi.example.com → VM-07", sub_size=8.0)

    # Interconnect
    arrow((33, 66), (37, 66), color=PALETTE["navy"], lw=1.5)
    arrow((63, 66), (67, 66), color=PALETTE["teal"], lw=1.5)

    # Middle Separator Banner
    box_(20, 44.5, 60, 4.0, fill=WHITE, edge=PALETTE["navy"], radius=0.6, lw=1.0)
    label(50, 46.5, "WireGuard Mesh: Transparent L4 TCP Passthrough (Port 443)", fontsize=8.6,
          color=PALETTE["navy"], weight="bold")

    # Outer Local Boundary Box (VPS Fleet)
    box_(4, 6, 92, 34, fill=PALETTE["slate_lt"], edge=PALETTE["slate"], radius=1.4, lw=1.5)
    label(50, 36.5, "LOCAL SERVICE ROUTERS (ON EACH VM: EXPERIMENTAL COOLIFY INTEGRATION)",
          fontsize=10.2, weight="bold", color=PALETTE["navy"])

    # Local Nodes
    nodes = [
        (7, 9, 26, 23, "VM-03 (Hetzner)", "Coolify Proxy (:443)", "app.example.com", "App Container"),
        (37, 9, 26, 23, "VM-07 (DigitalOcean)", "Coolify Proxy (:443)", "api.example.com", "API Container"),
        (67, 9, 26, 23, "VM-12 (Vultr / BareMetal)", "Coolify Proxy (:443)", "admin.example.com", "Admin Container"),
    ]

    for x, y, w, h, vm, proxy_title, dom, cont in nodes:
        box_(x, y, w, h, fill=PALETTE["white"], edge=PALETTE["teal"], radius=1.0)
        label(x + w/2, y + h - 2.5, vm, fontsize=9.2, weight="bold", color=PALETTE["navy"])
        box(x + 1.5, y + 9.5, w - 3, 8.5, proxy_title, fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
            fontsize=8.6, weight="bold", sub="local SSL / ACME + Traefik", sub_size=7.6)
        box(x + 1.5, y + 1.8, w - 3, 6.0, cont, fill=PALETTE["green_lt"], edge=PALETTE["green"],
            fontsize=8.6, weight="bold", sub=dom, sub_size=7.6)

    # Connections from Global Router through the banner down to local nodes
    arrow((50, 52), (50, 48.5), color=PALETTE["navy"], lw=1.4)
    arrow((20, 44.5), (20, 32), color=PALETTE["navy"], lw=1.4)
    arrow((50, 44.5), (50, 32), color=PALETTE["navy"], lw=1.4)
    arrow((80, 44.5), (80, 32), color=PALETTE["navy"], lw=1.4)

    footer(fig)
    return save(fig, out)


# --- 12. Managed Mode: Coolify Proxy Dynamic Routing -----------------------
def fig12_managed_mode(out):
    fig, ax = figure("Managed Mode", 11.0, 7.6)
    title(fig, "Managed Mode: Coolify Proxy Dynamic Routing",
          "Bridge acts as control-plane only: configures Traefik routing via file/HTTP provider. Zero hot-path involvement.",
          "12")

    # ===================== CONTROL PLANE (TOP) =====================
    box_(4, 52, 92, 36, fill=PALETTE["teal_lt"], edge=PALETTE["teal"], radius=1.4, lw=1.8)
    label(50, 85.5, "CONTROL PLANE (BRIDGE: NOT IN DATA PATH)", fontsize=11.0, weight="bold", color=PALETTE["navy"])
    label(50, 82.5, "Bridge manages routing state and writes Traefik dynamic configuration; never touches application traffic.",
          fontsize=8.4, color=PALETTE["slate_dk"], style="italic")

    # Domain Registry
    box(7, 60, 24, 16, "Domain Registry\n(SWIM Gossip)", fill=PALETTE["white"], edge=PALETTE["teal"],
        fontsize=9.2, weight="bold", sub="app.example.com → VM-03\napi.example.com → VM-07", sub_size=8.0)

    arrow((31, 68), (37, 68), color=PALETTE["teal"], lw=1.5)
    label(34, 70.8, "route\nupdate", fontsize=8.0, color=PALETTE["teal"], weight="bold")

    # Bridge Daemon
    box(37, 60, 24, 16, "Bridge Daemon\n(Control Plane)", fill=PALETTE["navy"], edge=PALETTE["navy"],
        tcolor=PALETTE["white"], fontsize=9.2, weight="bold",
        sub="translates routes → Traefik config", sub_size=8.0, sub_color=WHITE)

    arrow((61, 68), (67, 68), color=PALETTE["navy"], lw=1.5)
    label(64, 70.8, "writes\nconfig", fontsize=8.0, color=PALETTE["navy"], weight="bold")

    # Traefik Dynamic Config file
    box(67, 60, 26, 16, "Traefik Dynamic Config\n(/etc/traefik/dynamic/)", fill=PALETTE["white"], edge=PALETTE["slate"],
        fontsize=8.8, weight="bold", sub="bridge.yml: auto-generated\nrouters + services + TLS", sub_size=7.8)

    # Arrow from config down to Traefik (crosses boundary)
    arrow((80, 60), (80, 48.5), color=PALETTE["slate_dk"], lw=1.6, ls="--")
    label(88, 54, "file provider\nhot-reload", fontsize=8.2, color=PALETTE["slate_dk"], weight="bold")

    # ===================== SEPARATOR =====================
    box_(20, 44.5, 60, 4.0, fill=WHITE, edge=PALETTE["navy"], radius=0.6, lw=1.0)
    label(50, 46.5, "DATA PATH: Bridge has zero involvement below this line", fontsize=8.6,
          color=PALETTE["navy"], weight="bold")

    # ===================== DATA PATH (BOTTOM) =====================
    box_(4, 6, 92, 34, fill=PALETTE["green_lt"], edge=PALETTE["green"], radius=1.4, lw=1.5)
    label(50, 37, "DATA PATH (CLIENT → COOLIFY PROXY → SERVICE)", fontsize=10.5, weight="bold",
          color=PALETTE["navy"])

    # Client
    box(7, 12, 20, 16, "Client\n(Browser / API)", fill=PALETTE["white"], edge=PALETTE["teal"],
        fontsize=9.2, weight="bold", sub="HTTPS request", sub_size=8.0)

    arrow((27, 20), (37, 20), color=PALETTE["teal"], lw=1.6)
    label(32, 22.8, "HTTPS", fontsize=8.4, color=PALETTE["teal"], weight="bold")

    # Coolify Proxy / Traefik
    box(37, 12, 24, 16, "Coolify Proxy\n(Traefik :443)", fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
        fontsize=9.2, weight="bold", sub="TLS termination (Let's Encrypt)\nroutes via Bridge-injected config", sub_size=7.6)

    arrow((61, 20), (71, 20), color=PALETTE["green"], lw=1.6)
    label(66, 22.8, "WireGuard", fontsize=8.4, color=PALETTE["green"], weight="bold")

    # App Container
    box(71, 12, 22, 16, "App Container\n(VM-03 / Docker)", fill=PALETTE["green_lt"], edge=PALETTE["green"],
        fontsize=9.2, weight="bold", sub="10.8.0.3:3000\n(via WireGuard mesh)", sub_size=7.8)

    # Return arrow
    arrow((71, 15), (61, 15), color=PALETTE["green"], lw=1.2, ls="--")
    label(66, 13, "HTTP 200", fontsize=7.8, color=PALETTE["green"], weight="bold")
    arrow((37, 15), (27, 15), color=PALETTE["teal"], lw=1.2, ls="--")
    label(32, 13, "HTTPS resp", fontsize=7.8, color=PALETTE["teal"], weight="bold")

    footer(fig)
    return save(fig, out)


# --- 13. Concurrency & Dynamic Scaling Architecture -----------------------
def fig13_concurrency_scaling(out):
    fig, ax = figure("Concurrency Scaling", 11.0, 7.6)
    title(fig, "Adaptive Concurrency & Work-Stealing Architecture",
          "Dynamic Tokio worker task scaling driven by queue dwell latency with Pingora-style SPMC work-stealing.",
          "13")

    # ===================== TOP: INGRESS & DISPATCH LAYER =====================
    box_(4, 58, 92, 27, fill=PALETTE["slate_lt"], edge=PALETTE["slate"], radius=1.2, lw=1.5)
    label(50, 82.5, "INGRESS, ACCEPTOR & DISPATCH LAYER", fontsize=10.5, weight="bold", color=PALETTE["navy"])
    label(50, 79.8, "Single-Producer Acceptor Loop with Queue Dwell Time Tracker", fontsize=8.4, color=PALETTE["slate_dk"], style="italic")

    # 1. TCP Listener
    box_(7, 61, 23, 15, fill=PALETTE["teal_lt"], edge=PALETTE["teal"], radius=0.8, lw=1.2)
    label(18.5, 71.5, "TCP Listener", fontsize=9.2, weight="bold", color=PALETTE["navy"])
    label(18.5, 68.8, "(Port :443 / :80)", fontsize=8.0, color=PALETTE["navy"], weight="bold")
    label(18.5, 64.0, "Accepts client streams\nZero-blocking handoff", fontsize=7.6, color=PALETTE["slate_dk"], style="italic")

    arrow((30, 68.5), (37, 68.5), color=PALETTE["navy"], lw=1.6)
    label(33.5, 71.0, "stream", fontsize=8.0, color=PALETTE["navy"], weight="bold")

    # 2. Queue Dwell Monitor
    box_(37, 61, 26, 15, fill=PALETTE["white"], edge=PALETTE["navy"], radius=0.8, lw=1.2)
    label(50, 71.5, "Queue Dwell Monitor", fontsize=9.2, weight="bold", color=PALETTE["navy"])
    label(50, 68.8, "(Sliding 20-Req Window)", fontsize=8.0, color=PALETTE["navy"], weight="bold")
    label(50, 64.0, "Tags arrival: t_accepted\nTracks avg queue delay: Δt", fontsize=7.6, color=PALETTE["slate_dk"], style="italic")

    arrow((63, 68.5), (70, 68.5), color=PALETTE["navy"], lw=1.6)
    label(66.5, 71.0, "push", fontsize=8.0, color=PALETTE["navy"], weight="bold")

    # 3. Global SPMC Ingress Deque
    box_(70, 61, 23, 15, fill=PALETTE["white"], edge=PALETTE["teal"], radius=0.8, lw=1.2)
    label(81.5, 71.5, "Ingress Deque", fontsize=9.2, weight="bold", color=PALETTE["navy"])
    label(81.5, 68.8, "(crossbeam-deque)", fontsize=8.0, color=PALETTE["navy"], weight="bold")
    label(81.5, 64.0, "Lock-free SPMC buffer\nInjector for worker steal", fontsize=7.6, color=PALETTE["slate_dk"], style="italic")

    # ===================== MIDDLE: ELASTIC WORKER POOL =====================
    box_(4, 29, 92, 25, fill=PALETTE["teal_lt"], edge=PALETTE["teal"], radius=1.2, lw=1.8)
    label(50, 51.5, "ELASTIC TOKIO WORKER POOL (1 TO MAX_CONCURRENCY)", fontsize=10.5, weight="bold", color=PALETTE["navy"])
    label(50, 48.8, "Lightweight async tasks: workers pop local LIFO & steal FIFO when idle", fontsize=8.4, color=PALETTE["slate_dk"], style="italic")

    # Worker 1
    box_(7, 32, 24, 14, fill=PALETTE["green_lt"], edge=PALETTE["green"], radius=0.8, lw=1.2)
    label(19, 41.5, "Worker Task 1", fontsize=9.2, weight="bold", color=PALETTE["navy"])
    label(19, 38.8, "[Baseline Active]", fontsize=8.0, color=PALETTE["green"], weight="bold")
    label(19, 34.6, "SNI Parser + TCP Splice\nPops LIFO from local queue", fontsize=7.6, color=PALETTE["slate_dk"], style="italic")

    # Work steal arrow 1 <-> 2
    arrow((31, 40), (39, 40), color=PALETTE["crimson"], lw=1.4, ls="--")
    arrow((39, 38), (31, 38), color=PALETTE["crimson"], lw=1.4, ls="--")
    label(35, 43.5, "work steal", fontsize=7.8, color=PALETTE["crimson"], weight="bold")

    # Worker 2
    box_(39, 32, 24, 14, fill=PALETTE["white"], edge=PALETTE["teal"], radius=0.8, lw=1.2)
    label(51, 41.5, "Worker Task 2", fontsize=9.2, weight="bold", color=PALETTE["navy"])
    label(51, 38.8, "[Auto-Spawned]", fontsize=8.0, color=PALETTE["teal"], weight="bold")
    label(51, 34.6, "Spawned when Δt > 5ms\nSteals from Worker 1 / Ingress", fontsize=7.6, color=PALETTE["slate_dk"], style="italic")

    # Work steal arrow 2 <-> N
    arrow((63, 40), (71, 40), color=PALETTE["crimson"], lw=1.4, ls="--")
    arrow((71, 38), (63, 38), color=PALETTE["crimson"], lw=1.4, ls="--")
    label(67, 43.5, "work steal", fontsize=7.8, color=PALETTE["crimson"], weight="bold")

    # Worker N
    box_(71, 32, 23, 14, fill=PALETTE["white"], edge=PALETTE["slate"], radius=0.8, lw=1.2)
    label(82.5, 41.5, "Worker Task N ...", fontsize=9.2, weight="bold", color=PALETTE["navy"])
    label(82.5, 38.8, "[max_concurrency: 20]", fontsize=8.0, color=PALETTE["slate_dk"], weight="bold")
    label(82.5, 34.6, "Elastic limit (20 / Auto)\nReaps on 15s idle cooldown", fontsize=7.6, color=PALETTE["slate_dk"], style="italic")

    # Vertical Dispatch arrow from Ingress to Worker pool
    arrow((81.5, 61), (81.5, 46), color=PALETTE["teal"], lw=1.6)

    # ===================== BOTTOM: POLICY CARDS & LEGEND =====================
    # Card 1: Scale Up
    box_(4, 5, 21.5, 20, fill=PALETTE["crimson_lt"], edge=PALETTE["crimson"], radius=0.8, lw=1.2)
    label(14.75, 22.0, "Scale-Up Trigger", fontsize=9.0, weight="bold", color=PALETTE["crimson"])
    label(14.75, 19.8, "(Latency Bound)", fontsize=8.0, color=PALETTE["crimson"], weight="bold")
    ax.text(5.5, 17.2, "• Metric: Queue delay Δt\n• Threshold: Δt_dwell > 5ms\n• Buffer: Last 20 requests\n• Action: Spawn +1 Worker",
            fontsize=7.8, color=PALETTE["navy"], va="top", ha="left", linespacing=1.35, zorder=3)

    # Card 2: Work Stealing
    box_(27.5, 5, 21.5, 20, fill=PALETTE["white"], edge=PALETTE["teal"], radius=0.8, lw=1.2)
    label(38.25, 22.0, "Work-Stealing Policy", fontsize=9.0, weight="bold", color=PALETTE["navy"])
    label(38.25, 19.8, "(Pingora-Style)", fontsize=8.0, color=PALETTE["teal"], weight="bold")
    ax.text(28.5, 17.2, "• Owner: LIFO pop (cache)\n• Stealer: Steal FIFO top\n• Zero lock contention\n• Dynamic load balance",
            fontsize=7.8, color=PALETTE["slate_dk"], va="top", ha="left", linespacing=1.35, zorder=3)

    # Card 3: Scale Down
    box_(51, 5, 21.5, 20, fill=PALETTE["slate_lt"], edge=PALETTE["slate"], radius=0.8, lw=1.2)
    label(61.75, 22.0, "Scale-Down Reaper", fontsize=9.0, weight="bold", color=PALETTE["navy"])
    label(61.75, 19.8, "(Idle Cooldown)", fontsize=8.0, color=PALETTE["slate_dk"], weight="bold")
    ax.text(52.0, 17.2, "• Condition: Queue empty\n• Cooldown: Idle > 15s\n• Action: Graceful exit\n• Floor: Min 1 active worker",
            fontsize=7.8, color=PALETTE["slate_dk"], va="top", ha="left", linespacing=1.35, zorder=3)

    # Legend
    legend([
        (PALETTE["navy"], "Acceptor & Dwell Monitor"),
        (PALETTE["teal"], "SPMC Work-Stealing Deque"),
        (PALETTE["green"], "Baseline Active Worker"),
        (PALETTE["crimson"], "Work-Stealing Path", "line"),
    ], x=74.5, y=5.0, w=21.5, h=20.0, title="Component Legend")

    footer(fig)
    return save(fig, out)


if __name__ == "__main__":
    out = Path(__file__).parent.parent / "assets"
    out.mkdir(exist_ok=True)
    for fn, n in [(fig09_proxy_modes,          "09_proxy_modes_architecture.png"),
                  (fig10_seq_sni_handoff,      "10_seq_sni_handoff_routing.png"),
                  (fig11_separation,           "11_proxy_handoff_separation.png"),
                  (fig12_managed_mode,         "12_managed_mode_architecture.png"),
                  (fig13_concurrency_scaling,  "13_concurrency_scaling_architecture.png")]:
        p = fn(str(out / n))
        print("wrote", p)


