"""Artifacts: system context, C4 architecture, deployment diagram.

01 — System Context Diagram
02 — High-Level Architecture (C4 L1+L2)
04 — Deployment Diagram (mesh + Cloudflare edge)
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import networkx as nx  # noqa: F401

from style import (arrow, box, box_, figure, footer, label, legend, PALETTE, save,
                   title, circle, WHITE, HAIRLINE)


# --- 01. System Context ---------------------------------------------------
def fig01_system_context(out):
    fig, ax = figure("System Context", 11.0, 7.0)
    title(fig, "System Context", "BRIDGE sits between the public internet and "
          "your VPS fleet; its only external dependency is Cloudflare's edge.",
          "01")

    # Ingress / Clients
    box(38, 80, 24, 8.0, "Clients / Public Internet", fill=PALETTE["teal_lt"],
        edge=PALETTE["teal"], fontsize=10.5, weight="bold",
        sub="HTTPS / HTTP requests", sub_size=8.8)

    # External dependency: Cloudflare
    box(2.5, 48, 21.5, 9.5, "Cloudflare Edge\n(Anycast)",
        fill=PALETTE["navy"], edge=PALETTE["navy"], tcolor=PALETTE["white"],
        fontsize=10.0, weight="bold", sub="external entry dependency", sub_size=8.5, sub_color=WHITE)

    # Central BRIDGE system
    sys_y = 44
    box(34, sys_y, 34, 18, "BRIDGE",
        fill=PALETTE["white"], edge=PALETTE["navy"], tcolor=PALETTE["navy"],
        radius=1.6, lw=2.0, fontsize=14.5, weight="bold",
        sub="Builds mesh · Routes · Isolates · Discovers · Guards · Elects", sub_size=8.8)

    # Surrounding auxiliary actors
    box(75, 56, 21, 9.0, "Provider APIs\n(Hetzner, DO...)",
        fill=PALETTE["white"], edge=PALETTE["teal"], fontsize=9.2, weight="bold",
        sub="floating IP handoff", radius=1.0, sub_size=8.2)
    box(75, 43, 21, 9.0, "ACME / DV CA\n(Let's Encrypt)",
        fill=PALETTE["white"], edge=PALETTE["teal"], fontsize=9.2, weight="bold",
        sub="automated TLS certs", radius=1.0, sub_size=8.2)

    # Peers (VPS fleet)
    peers = [
        (22, 17, "VPS A\n(Hetzner)"),
        (40, 17, "VPS B\n(DigitalOcean)"),
        (58, 17, "VPS C\n(Vultr)"),
        (76, 17, "VPS D\n(Homelab / Bare Metal)"),
    ]
    for px, py, pl in peers:
        box(px, py, 16.5, 10.0, pl, fill=PALETTE["slate_lt"], edge=PALETTE["slate"],
            fontsize=9.2, weight="bold", sub="BRIDGE daemon", sub_size=8.2)

    # Connections
    arrow((50, 80), (51, 62), color=PALETTE["slate_dk"], lw=1.5)
    label(59.0, 71, "HTTPS traffic", fontsize=9.2, color=PALETTE["slate_dk"], weight="bold")

    arrow((24, 52.5), (34, 52.5), color=PALETTE["navy"], lw=1.5)
    label(29.0, 55.5, "QUIC tunnel", fontsize=8.8, color=PALETTE["navy"], weight="bold")

    arrow((68, 60.5), (75, 60.5), color=PALETTE["teal"], lw=1.3)
    arrow((68, 47.5), (75, 47.5), color=PALETTE["teal"], lw=1.3)

    for px, _, _ in peers:
        arrow((50, sys_y), (px+8.25, 27.0), color=PALETTE["slate"],
              rad=0.0, lw=1.2)

    # Mesh label banner pill
    box_(28, 33.5, 44, 3.8, fill=WHITE, edge=HAIRLINE, radius=0.6)
    label(50, 35.4, "WireGuard Mesh & SWIM Gossip Interconnect",
          fontsize=8.8, color=PALETTE["slate_dk"], weight="bold")

    legend([
        (PALETTE["navy"], "External actor / dependency"),
        (PALETTE["teal"], "Auxiliary provider service"),
        (PALETTE["slate"], "Internal fleet node (VPS)"),
        (PALETTE["white"], "BRIDGE core process"),
    ], x=2.5, y=3.2, w=27, h=13.0, title="System Legend")

    footer(fig)
    return save(fig, out)


# --- 02. C4 architecture --------------------------------------------------
def fig02_architecture_c4(out):
    fig, ax = figure("Architecture (C4)", 11.0, 7.6)
    title(fig, "High-Level Architecture (C4 L1+L2)",
          "The active leader terminates ingress and routes traffic; peers replicate state over WireGuard.",
          "02")

    # Top band — public entry (positioned below title)
    box(37, 78.5, 26, 8.0, "Cloudflare Anycast Edge", fill=PALETTE["navy"],
        edge=PALETTE["navy"], tcolor=PALETTE["white"], fontsize=10.5,
        weight="bold", sub="stable public IP (or floating IP in Mode 2)",
        sub_size=8.5, sub_color=WHITE)

    # Leader node container (drawn as plain box to prevent center text collision)
    box_(27, 50.5, 46, 21.5, fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
         radius=1.4, lw=2.0)
    label(50, 69.5, "ACTIVE LEADER (Node A)", fontsize=11.5, weight="bold",
          color=PALETTE["navy"])
    label(50, 66.8, "holds active tunnel · routes inbound fleet traffic",
          fontsize=8.8, color=PALETTE["slate_dk"], style="italic")

    # Leader internals
    box(29.5, 52.5, 11.5, 11.5, "cloudflared\n(active)", fill=PALETTE["white"],
        edge=PALETTE["teal"], fontsize=9.0, weight="bold", radius=0.8, sub="QUIC tunnel", sub_size=8.0)
    box(44.25, 52.5, 11.5, 11.5, "BRIDGE\ndaemon", fill=PALETTE["white"],
        edge=PALETTE["navy"], fontsize=9.2, radius=0.8, weight="bold",
        sub="SWIM + election", sub_size=8.0)
    box(59.0, 52.5, 11.5, 11.5, "proxy\n(Hyper)", fill=PALETTE["white"],
        edge=PALETTE["teal"], fontsize=9.0, weight="bold", radius=0.8, sub="L7 router", sub_size=8.0)

    # Mesh label banner pill
    box_(20, 43.2, 60, 3.8, fill=WHITE, edge=HAIRLINE, radius=0.6)
    label(50, 45.1, "WireGuard Mesh (L3 encrypted) · SWIM Gossip: routing table + membership",
          fontsize=8.6, color=PALETTE["slate_dk"], weight="bold")

    # Peers
    peers = [
        (9.5, 15.5, 17.5, 23.0, "Node B", "svc-api"),
        (28.5, 15.5, 17.5, 23.0, "Node C", "svc-db"),
        (54.0, 15.5, 17.5, 23.0, "Node D", "svc-web"),
        (73.0, 15.5, 17.5, 23.0, "Node E", "svc-cache"),
    ]
    for px, py, pw, ph, pname, svc in peers:
        box_(px, py, pw, ph, fill=PALETTE["slate_lt"], edge=PALETTE["slate"],
             radius=1.2)
        label(px + pw/2, py + ph - 2.8, pname, fontsize=10.0, weight="bold",
              color=PALETTE["navy"])
        label(px + pw/2, py + ph - 5.0, "standby daemon",
              fontsize=8.2, color=PALETTE["slate_dk"], style="italic")
        box(px + 1.5, py + 10.0, pw - 3, 7.0, "BRIDGE daemon", fill=PALETTE["white"],
            edge=PALETTE["slate"], fontsize=8.8, weight="bold", radius=0.6, sub="SWIM gossip", sub_size=7.8)
        box(px + 1.5, py + 1.8, pw - 3, 7.0, svc, fill=PALETTE["white"],
            edge=PALETTE["teal"], fontsize=8.8, weight="bold", radius=0.6, sub="local service", sub_size=7.8)

    # Cloudflare tunnel arrow
    arrow((50, 78.5), (50, 72.0), color=PALETTE["navy"], lw=1.6)
    label(62, 75.2, "outbound tunnel · QUIC", fontsize=8.8,
          color=PALETTE["navy"], weight="bold")

    # Heartbeat and mesh links
    arrow((50, 50.5), (50, 47.0), color=PALETTE["teal"], lw=1.4)
    label(58.5, 48.8, "heartbeat (TTL=5s)", fontsize=8.2, color=PALETTE["teal"], weight="bold")

    for px, py, pw, ph, _, _ in peers:
        arrow((50, 43.2), (px + pw/2, py + ph), color=PALETTE["slate"], lw=1.2)

    # Legend
    legend([
        (PALETTE["navy"], "External / public entry"),
        (PALETTE["teal"], "Active leader node"),
        (PALETTE["white"], "Daemon / service process"),
        (PALETTE["slate_lt"], "Peer node (standby)"),
    ], x=2.5, y=3.2, w=27, h=12.5, title="Reading guide")

    footer(fig)
    return save(fig, out)


# --- 04. Deployment diagram -----------------------------------------------
def fig04_deployment(out):
    fig, ax = figure("Deployment", 11.0, 7.6)
    title(fig, "Deployment Diagram",
          "Nodes across providers join a single WireGuard mesh; the active "
          "leader holds the Cloudflare Tunnel, peers hold standby tunnels.",
          "04")

    # Cloudflare cloud (top, positioned below title)
    box(37, 78.5, 26, 8.5, "Cloudflare Edge", fill=PALETTE["navy"],
        edge=PALETTE["navy"], tcolor=PALETTE["white"], fontsize=10.5,
        weight="bold", sub="Anycast · QUIC tunnels", sub_size=8.5, sub_color=WHITE)

    # VPS boxes — providers (2 rows of 3)
    nodes = [
        (10, 51.5, "Hetzner FSN1", "Node A · LEADER", PALETTE["teal_lt"],
         PALETTE["teal"]),
        (39, 51.5, "DigitalOcean FRA1", "Node B · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (68, 51.5, "Vultr FRA", "Node C · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (10, 16.5, "On-Prem / Homelab", "Node D · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (39, 16.5, "Hetzner HEL1", "Node E · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (68, 16.5, "AWS eu-central-1", "Node F · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
    ]

    # Containers inside each VPS
    for x, y, prov, name, fi, ed in nodes:
        box_(x, y, 22, 19.5, fill=PALETTE["white"], edge=ed, radius=1.4)
        label(x + 11, y + 16.8, prov, fontsize=9.5, weight="bold",
              color=PALETTE["slate_dk"])
        label(x + 11, y + 14.0, name, fontsize=8.5, color=ed, weight="bold")
        box(x + 1.2, y + 2.0, 9.4, 10.0, "WireGuard", fill=fi, edge=ed, fontsize=8.8,
            weight="bold", radius=0.6, sub="L3 mesh", sub_size=7.8)
        box(x + 11.4, y + 2.0, 9.4, 10.0, "BRIDGE", fill=fi, edge=ed, fontsize=8.8,
            weight="bold", radius=0.6, sub="daemon", sub_size=7.8)

    # Active tunnel — leader only
    arrow((43, 78.5), (21, 71.0), color=PALETTE["navy"], lw=1.8)
    label(26, 76.5, "active QUIC tunnel", fontsize=8.8, color=PALETTE["navy"],
          weight="bold")

    # Mesh links — clean grid interconnect
    # Horizontal links
    arrow((32, 61.2), (39, 61.2), color=PALETTE["slate"], lw=1.1, ls="--")
    arrow((61, 61.2), (68, 61.2), color=PALETTE["slate"], lw=1.1, ls="--")
    arrow((32, 26.2), (39, 26.2), color=PALETTE["slate"], lw=1.1, ls="--")
    arrow((61, 26.2), (68, 26.2), color=PALETTE["slate"], lw=1.1, ls="--")
    # Vertical links
    arrow((21, 51.5), (21, 36.0), color=PALETTE["slate"], lw=1.1, ls="--")
    arrow((50, 51.5), (50, 36.0), color=PALETTE["slate"], lw=1.1, ls="--")
    arrow((79, 51.5), (79, 36.0), color=PALETTE["slate"], lw=1.1, ls="--")

    # Center banner pill
    box_(22, 42.0, 56, 4.0, fill=WHITE, edge=HAIRLINE, radius=0.6)
    label(50, 44.0, "WireGuard Mesh · SWIM Gossip (Full Peer Interconnect)",
          fontsize=8.8, color=PALETTE["slate_dk"], weight="bold")

    legend([
        (PALETTE["navy"], "Cloudflare Anycast edge"),
        (PALETTE["teal"], "Active leader node"),
        (PALETTE["slate"], "Peer standby node"),
        (PALETTE["slate"], "WireGuard encrypted mesh", "line"),
    ], x=2.5, y=3.0, w=27, h=12.5, title="Deployment legend")

    footer(fig)
    return save(fig, out)


if __name__ == "__main__":
    out = Path(__file__).parent.parent / "assets"
    out.mkdir(exist_ok=True)
    for fn, n in [(fig01_system_context, "01_system_context.png"),
                  (fig02_architecture_c4, "02_architecture_c4.png"),
                  (fig04_deployment,      "04_deployment_diagram.png")]:
        p = fn(out / n)
        print("wrote", p)


