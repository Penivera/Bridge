"""Artifacts: UML-style sequence diagrams for BRIDGE.

05 — Leader Failover sequence
06 — New Node Bootstrap sequence
07 — Request Routing + consistent-hash sequence
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from style import PALETTE, figure, footer, save, title

import matplotlib.patches as mp
import matplotlib.pyplot as plt


# --- Sequence diagram scaffolding -----------------------------------------
def _lifelines(fig, actors, top=82, bottom=12, box_w=14, box_h=5.2):
    """Draw vertical lifelines for ``actors`` = list of (name, x)."""
    ax = fig.axes[0]
    for name, x in actors:
        ax.plot([x, x], [bottom, top], color=PALETTE["hairline"], lw=1.0,
                zorder=1)
        # actor header box centered at x
        bx = x - box_w / 2
        ax.add_patch(mp.FancyBboxPatch(
            (bx, top), box_w, box_h, boxstyle="round,pad=0.02,rounding_size=0.8",
            linewidth=1.2, edgecolor=PALETTE["navy"],
            facecolor=PALETTE["navy"], zorder=3))
        ax.text(x, top + box_h / 2, name, ha="center", va="center", fontsize=9.0,
                weight="bold", color=PALETTE["white"], zorder=4)


def _msg(fig, x1, x2, y, label, *, dashed=False, color=None, fs=8.6, rad=0.0):
    """Arrow with a label between two lifelines at vertical position y."""
    ax = fig.axes[0]
    color = color or PALETTE["slate_dk"]
    style = "<|-|>" if abs(x1 - x2) < 0.5 else "-|>"
    if abs(x1 - x2) < 0.5:
        # self-message: small loop
        verts = [(x1, y), (x1 + 4, y), (x1 + 4, y - 2), (x1, y - 2)]
        ax.add_patch(mp.FancyArrowPatch(
            (x1 + 4, y - 2), (x1, y - 2), arrowstyle="-|>",
            color=color, lw=1.2, connectionstyle="arc3,rad=0",
            mutation_scale=11, zorder=4))
        ax.plot([x1, x1 + 4, x1 + 4, x1], [y, y, y - 2, y - 2],
                color=color, lw=1.2, zorder=4)
    else:
        a = mp.FancyArrowPatch((x1, y), (x2, y), arrowstyle=style,
                               color=color, lw=1.2, linestyle="--" if dashed else "-",
                               connectionstyle=f"arc3,rad={rad}",
                               mutation_scale=11, zorder=4)
        ax.add_patch(a)
    mx = (x1 + x2) / 2 + (rad * 30 if rad else 0)
    ax.text(mx, y + 1.2, label, ha="center", va="bottom", fontsize=fs,
            color=PALETTE["slate_dk"], style="italic", zorder=5)


def _note(fig, x, y, text, *, w=18, h=3.4, color=None, fs=7.8):
    """Sticky-note rectangle on a lifeline."""
    ax = fig.axes[0]
    color = color or PALETTE["amber"]
    ax.add_patch(mp.FancyBboxPatch(
        (x - w/2, y), w, h, boxstyle="round,pad=0.02,rounding_size=0.5",
        linewidth=0.8, edgecolor=color, facecolor=PALETTE["white"], zorder=2))
    ax.text(x, y + h/2, text, ha="center", va="center", fontsize=fs,
            color=PALETTE["slate_dk"], style="italic", zorder=3)


def _activation(fig, x, y_top, y_bot, color=None):
    """Thin activation bar on a lifeline."""
    ax = fig.axes[0]
    color = color or PALETTE["teal"]
    ax.add_patch(mp.Rectangle((x - 0.8, y_bot), 1.6, y_top - y_bot,
                              facecolor=color, edgecolor=color, zorder=3,
                              alpha=0.4))


def _timer(fig, y, label="t = ?"):
    """Thin horizontal time axis label on the left."""
    ax = fig.axes[0]
    ax.text(1.5, y, label, fontsize=7.4, color=PALETTE["muted"],
            ha="left", va="center", style="italic")


# --- 05. Leader failover sequence -----------------------------------------
def fig05_leader_failover(out):
    fig, ax = figure("Leader Failover", 13, 9.2)
    title(fig, "Sequence — Leader Failover",
          "Heartbeats die → failure detection → bully election → quorum reached → tunnel handoff.",
          "05")

    actors = [("Client", 10), ("Cloudflare", 27),
              ("Node A (Leader)", 45), ("Node B", 62),
              ("Node C", 78), ("Node D (New)", 93)]
    _lifelines(fig, actors, top=82, bottom=9, box_w=13.5)

    # Initial healthy state
    y = 76
    _timer(fig, y, "T₀ (normal)")
    _msg(fig, 10, 45, y, "HTTPS request")
    _activation(fig, 45, y, y - 4)
    _msg(fig, 45, 10, y - 4, "200 OK (served by Node A)")

    # Heartbeat & Leader crash
    y = 66
    _timer(fig, y, "T₀+1s (crash)")
    _msg(fig, 45, 62, y, "heartbeat ping")
    _msg(fig, 45, 78, y - 3, "heartbeat ping")
    _note(fig, 36, y - 4.5, "Node A crashes", color=PALETTE["crimson"], w=14, h=3.0)
    ax.plot([45, 45], [9, y - 1], color=PALETTE["crimson"], lw=1.2, zorder=2)
    ax.text(45, y + 0.8, "✗", ha="center", va="center", fontsize=12,
            color=PALETTE["crimson"], weight="bold", zorder=4)

    # Failure detection
    y = 54
    _timer(fig, y, "T₀+5s (TTL expired)")
    _note(fig, 62, y + 3.2, "missed 3 heartbeats (TTL=5s)",
          color=PALETTE["crimson"], w=22, h=3.0)
    _msg(fig, 62, 78, y, "indirect probe of Node A", color=PALETTE["crimson"])
    _msg(fig, 78, 62, y - 4, "no reply / probe failed", color=PALETTE["crimson"], dashed=True)

    # Bully election
    y = 43
    _timer(fig, y, "T₀+8s (election)")
    _msg(fig, 62, 93, y, "ELECTION (Node B → highest ID Node D)")
    _msg(fig, 93, 62, y - 4, "OK (Node D coordinates)", dashed=True)

    # Quorum and coordinator announcement
    y = 32
    _timer(fig, y, "T₀+10s (quorum)")
    _msg(fig, 93, 62, y, "COORDINATOR: Node D is new leader", color=PALETTE["teal"])
    _msg(fig, 93, 78, y - 4, "ack (quorum ⌈n/2⌉=3 reached)", color=PALETTE["teal"], dashed=True)

    # Tunnel handoff
    y = 21
    _timer(fig, y, "T₀+12s (handoff)")
    _msg(fig, 93, 27, y, "acquire Cloudflare tunnel (warm start)",
         color=PALETTE["navy"])
    _activation(fig, 93, y, y - 7.5, color=PALETTE["navy"])
    _msg(fig, 27, 93, y - 4.5, "tunnel established to Node D", color=PALETTE["navy"],
         dashed=True)

    # Client retry & service recovery
    y = 12
    _timer(fig, y, "T₀+15s (restored)")
    _msg(fig, 10, 27, y, "client retry → Cloudflare")
    _msg(fig, 27, 93, y - 3.2, "HTTPS request routed to Node D")
    _activation(fig, 93, y - 3.2, y - 6.5, color=PALETTE["navy"])
    _msg(fig, 93, 10, y - 6.5, "200 OK (served by Node D)",
         color=PALETTE["green"])

    footer(fig)
    return save(fig, out)


# --- 06. New node bootstrap -----------------------------------------------
def fig06_node_bootstrap(out):
    fig, ax = figure("Node Bootstrap", 12, 9.0)
    title(fig, "Sequence — New Node Bootstrap",
          "A fresh node joins via seed endpoint, syncs peer list, and converges across fleet in O(log n).",
          "06")

    actors = [("New Node N", 10), ("Seed S", 32),
              ("WireGuard", 52), ("Peer B", 72), ("Peer C", 90)]
    _lifelines(fig, actors, top=82, bottom=9, box_w=14)

    # Boot & WireGuard handshake
    y = 76
    _timer(fig, y, "T₀ (boot)")
    _msg(fig, 10, 32, y, "read bridge.toml (seed endpoint + pubkey)")

    y = 70
    _timer(fig, y, "T₀+0.3s")
    _msg(fig, 10, 52, y, "WireGuard handshake (seed peer)", color=PALETTE["teal"])
    _msg(fig, 52, 10, y - 3.5, "tunnel interface wg0 established", color=PALETTE["teal"],
         dashed=True)
    _activation(fig, 10, y - 3.5, 12, color=PALETTE["teal"])

    # gRPC peer discovery
    y = 59
    _timer(fig, y, "T₀+0.8s")
    _msg(fig, 10, 32, y, "gRPC: GetPeerList()")
    _activation(fig, 32, y, y - 4)
    _msg(fig, 32, 10, y - 4, "peer list: {id, pubkey, endpoint, last_seen}",
         dashed=True)
    _note(fig, 21, y - 7.8, "Tonic gRPC over WireGuard",
          color=PALETTE["amber"], w=19, h=3.0)

    # Add dynamic peers
    y = 45
    _timer(fig, y, "T₀+1.5s")
    _msg(fig, 10, 52, y, "add WireGuard peers dynamically", color=PALETTE["teal"])

    # Gossip broadcast
    y = 39
    _timer(fig, y, "T₀+2.0s")
    _msg(fig, 32, 72, y, "gossip: NodeJoined(Node N)", color=PALETTE["teal"])
    _msg(fig, 32, 90, y - 4, "gossip: NodeJoined(Node N)", color=PALETTE["teal"])

    # Routing table digest exchange
    y = 28
    _timer(fig, y, "T₀+2.6s")
    _msg(fig, 72, 10, y, "mesh exchange: routing table digest")
    _msg(fig, 90, 10, y - 4.5, "route sync confirmed", dashed=True)

    # Active SWIM membership
    y = 16
    _timer(fig, y, "T₀+3.0s (ready)")
    _msg(fig, 10, 32, y, "NodeJoined → NodeAlive (SWIM confirmed)", color=PALETTE["green"])
    _note(fig, 21, y - 5.5, "convergence in ~log_f(n) rounds",
          color=PALETTE["green"], w=21, h=3.0)

    footer(fig)
    return save(fig, out)


# --- 07. Request routing + consistent hashing -----------------------------
def fig07_request_routing(out):
    fig, ax = figure("Request Routing", 12, 9.4)
    title(fig, "Sequence — Request Routing & Consistent Hashing",
          "Leader hashes client IP/host, maps to hash ring token, and proxies to healthy backend.",
          "07")

    actors = [("Client", 10), ("Leader Node", 30),
              ("Hash Ring", 50), ("Backend B1", 70), ("Backend B2", 90)]
    _lifelines(fig, actors, top=82, bottom=9, box_w=14)

    # Request 1 (Client A → Backend B1)
    y = 76
    _timer(fig, y, "Req #1 (IP_A)")
    _msg(fig, 10, 30, y, "GET /api/v1/resource")
    _activation(fig, 30, y, 48)

    _msg(fig, 30, 50, y - 4.5, "SHA256(src_ip_A + host)")
    _activation(fig, 50, y - 4.5, y - 9)
    _msg(fig, 50, 30, y - 9,
         "token lookup → Backend B1",
         dashed=True)
    _note(fig, 50, y - 13.5,
          "ring: 360 vnodes · O(log v) lookup",
          color=PALETTE["amber"], w=24, h=3.0)

    _msg(fig, 30, 70, y - 18, "proxy to B1 (token: B1-240)",
         color=PALETTE["teal"])
    _activation(fig, 70, y - 18, y - 23, color=PALETTE["teal"])
    _msg(fig, 70, 30, y - 23, "200 OK + payload", color=PALETTE["teal"],
         dashed=True)
    _msg(fig, 30, 10, y - 27, "200 OK (proxied response)", color=PALETTE["green"])

    # Request 2 (Client B → Backend B2)
    y = 42
    _timer(fig, y, "Req #2 (IP_B)")
    _msg(fig, 10, 30, y, "GET /api/v1/resource (different IP)")
    _activation(fig, 30, y, 16)

    _msg(fig, 30, 50, y - 4.5, "SHA256(src_ip_B + host)")
    _activation(fig, 50, y - 4.5, y - 9)
    _msg(fig, 50, 30, y - 9, "token lookup → Backend B2", dashed=True)
    _msg(fig, 30, 90, y - 14, "proxy to B2 (token: B2-320)",
         color=PALETTE["teal"])
    _activation(fig, 90, y - 14, y - 19, color=PALETTE["teal"])
    _msg(fig, 90, 30, y - 19, "200 OK + payload", color=PALETTE["teal"],
         dashed=True)
    _msg(fig, 30, 10, y - 23, "200 OK (proxied response)", color=PALETTE["green"])

    _note(fig, 70, y - 28.5, "session affinity: same client IP routes to same backend",
          color=PALETTE["amber"], w=27, h=3.2)

    footer(fig)
    return save(fig, out)




if __name__ == "__main__":
    out = Path(__file__).parent.parent / "assets"
    out.mkdir(exist_ok=True)
    for fn, n in [(fig05_leader_failover, "05_seq_leader_failover.png"),
                  (fig06_node_bootstrap,   "06_seq_node_bootstrap.png"),
                  (fig07_request_routing,  "07_seq_request_routing.png")]:
        p = fn(str(out / n))
        print("wrote", p)

