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
    fig, ax = figure("System Context", 12, 7.5)
    title(fig, "System Context", "BRIDGE sits between the public internet and "
          "your VPS fleet — its only external dependency is Cloudflare's edge.",
          "01")

    # Ingress / Clients
    box(38, 80, 24, 7.5, "Clients / Public Internet", fill=PALETTE["teal_lt"],
        edge=PALETTE["teal"], fontsize=9.5, weight="bold",
        sub="HTTPS / HTTP requests", sub_size=7.4)

    # External dependency: Cloudflare
    box(4, 48, 22, 9, "Cloudflare Edge\n(Anycast)",
        fill=PALETTE["navy"], edge=PALETTE["navy"], tcolor=PALETTE["white"],
        fontsize=9.2, weight="bold", sub="external entry dependency", sub_size=7.4)

    # Central BRIDGE system
    sys_y = 44
    box(33, sys_y, 34, 18, "BRIDGE",
        fill=PALETTE["white"], edge=PALETTE["navy"], tcolor=PALETTE["navy"],
        radius=1.6, lw=2.0, fontsize=13.5, weight="bold",
        sub="Builds mesh · Routes · Isolates · Discovers · Guards · Elects", sub_size=7.2)

    # Surrounding auxiliary actors
    box(76, 56, 20, 8.5, "Provider APIs\n(Hetzner, DO...)",
        fill=PALETTE["white"], edge=PALETTE["teal"], fontsize=8.8,
        sub="floating IP handoff", radius=1.0, sub_size=7.2)
    box(76, 43, 20, 8.5, "ACME / DV CA\n(Let's Encrypt)",
        fill=PALETTE["white"], edge=PALETTE["teal"], fontsize=8.8,
        sub="automated TLS certs", radius=1.0, sub_size=7.2)

    # Peers (VPS fleet)
    peers = [
        (22, 17, "VPS A\n(Hetzner)"),
        (40, 17, "VPS B\n(DigitalOcean)"),
        (58, 17, "VPS C\n(Vultr)"),
        (76, 17, "VPS D\n(Homelab / Bare Metal)"),
    ]
    for px, py, pl in peers:
        box(px, py, 16, 9.5, pl, fill=PALETTE["slate_lt"], edge=PALETTE["slate"],
            fontsize=8.5, sub="BRIDGE daemon", sub_size=7.2)

    # Connections
    arrow((50, 80), (50, 62), color=PALETTE["slate_dk"], lw=1.4)
    label(57.5, 71, "HTTPS traffic", fontsize=8.0, color=PALETTE["slate_dk"])

    arrow((26, 52.5), (33, 52.5), color=PALETTE["navy"], lw=1.4)
    label(29.5, 55.5, "QUIC tunnel", fontsize=7.6, color=PALETTE["navy"])

    arrow((67, 60.2), (76, 60.2), color=PALETTE["teal"], lw=1.2)
    arrow((67, 47.2), (76, 47.2), color=PALETTE["teal"], lw=1.2)

    for px, _, _ in peers:
        arrow((50, sys_y), (px+8, 26.5), color=PALETTE["slate"],
              rad=0.0, lw=1.0)

    # Mesh label banner pill
    box_(30, 33.5, 40, 3.4, fill=WHITE, edge=HAIRLINE, radius=0.5)
    label(50, 35.2, "WireGuard Mesh & SWIM Gossip Interconnect",
          fontsize=7.8, color=PALETTE["slate_dk"], weight="bold")

    legend([
        (PALETTE["navy"], "External actor / dependency"),
        (PALETTE["teal"], "Auxiliary provider service"),
        (PALETTE["slate"], "Internal fleet node (VPS)"),
        (PALETTE["white"], "BRIDGE core process"),
    ], x=2.5, y=3.8, w=24, h=11.8, title="Legend")

    footer(fig)
    return save(fig, out)


# --- 02. C4 architecture --------------------------------------------------
def fig02_architecture_c4(out):
    fig, ax = figure("Architecture (C4)", 13, 8.8)
    title(fig, "High-Level Architecture (C4 L1+L2)",
          "The active leader terminates ingress and routes traffic; peers replicate state over WireGuard.",
          "02")

    # Top band — public entry (positioned below title)
    box(38, 79, 24, 7.5, "Cloudflare Anycast Edge", fill=PALETTE["navy"],
        edge=PALETTE["navy"], tcolor=PALETTE["white"], fontsize=9.5,
        weight="bold", sub="stable public IP (or floating IP in Mode 2)",
        sub_size=7.2)

    # Leader node container (drawn as plain box to prevent center text collision)
    box_(28, 51, 44, 21, fill=PALETTE["teal_lt"], edge=PALETTE["teal"],
         radius=1.4, lw=2.0)
    label(50, 69.5, "ACTIVE LEADER (Node A)", fontsize=11, weight="bold",
          color=PALETTE["navy"])
    label(50, 66.8, "holds active tunnel · routes inbound fleet traffic",
          fontsize=7.8, color=PALETTE["slate_dk"], style="italic")

    # Leader internals
    box(30.5, 53, 11, 11, "cloudflared\n(active)", fill=PALETTE["white"],
        edge=PALETTE["teal"], fontsize=8.0, radius=0.8, sub="QUIC tunnel", sub_size=7.0)
    box(44.5, 53, 11, 11, "BRIDGE\ndaemon", fill=PALETTE["white"],
        edge=PALETTE["navy"], fontsize=8.4, radius=0.8, weight="bold",
        sub="SWIM + election", sub_size=7.0)
    box(58.5, 53, 11, 11, "proxy\n(Hyper)", fill=PALETTE["white"],
        edge=PALETTE["teal"], fontsize=8.0, radius=0.8, sub="L7 router", sub_size=7.0)

    # Mesh label banner pill
    box_(22, 43.5, 56, 3.4, fill=WHITE, edge=HAIRLINE, radius=0.5)
    label(50, 45.2, "WireGuard Mesh (L3 encrypted) · SWIM Gossip: routing table + membership",
          fontsize=8.0, color=PALETTE["slate_dk"], weight="bold")

    # Peers
    peers = [
        (10, 16, 17, 22, "Node B", "svc-api"),
        (28.5, 16, 17, 22, "Node C", "svc-db"),
        (54.5, 16, 17, 22, "Node D", "svc-web"),
        (73, 16, 17, 22, "Node E", "svc-cache"),
    ]
    for px, py, pw, ph, pname, svc in peers:
        box_(px, py, pw, ph, fill=PALETTE["slate_lt"], edge=PALETTE["slate"],
             radius=1.2)
        label(px + pw/2, py + ph - 2.5, pname, fontsize=9.5, weight="bold",
              color=PALETTE["navy"])
        label(px + pw/2, py + ph - 4.5, "standby daemon",
              fontsize=7.2, color=PALETTE["muted"], style="italic")
        box(px + 1.5, py + 9.5, pw - 3, 6.5, "BRIDGE daemon", fill=PALETTE["white"],
            edge=PALETTE["slate"], fontsize=7.6, radius=0.6, sub="SWIM gossip", sub_size=6.6)
        box(px + 1.5, py + 1.8, pw - 3, 6.5, svc, fill=PALETTE["white"],
            edge=PALETTE["teal"], fontsize=7.8, radius=0.6, sub="local service", sub_size=6.6)

    # Cloudflare tunnel arrow
    arrow((50, 79), (50, 72), color=PALETTE["navy"], lw=1.6)
    label(62, 75.5, "outbound tunnel · QUIC", fontsize=7.8,
          color=PALETTE["navy"])

    # Heartbeat and mesh links
    arrow((50, 51), (50, 47), color=PALETTE["teal"], lw=1.4)
    label(58.5, 49.0, "heartbeat (TTL=5s)", fontsize=7.4, color=PALETTE["teal"])

    for px, py, pw, ph, _, _ in peers:
        arrow((50, 43.5), (px + pw/2, py + ph), color=PALETTE["slate"], lw=1.0)

    # Legend
    legend([
        (PALETTE["navy"], "External / public entry"),
        (PALETTE["teal"], "Active leader node"),
        (PALETTE["white"], "Daemon / service process"),
        (PALETTE["slate_lt"], "Peer node (standby)"),
    ], x=2.5, y=3.6, w=24, h=11.5, title="Reading guide")

    footer(fig)
    return save(fig, out)


# --- 04. Deployment diagram -----------------------------------------------
def fig04_deployment(out):
    fig, ax = figure("Deployment", 13, 8.8)
    title(fig, "Deployment Diagram",
          "Nodes across providers join a single WireGuard mesh; the active "
          "leader holds the Cloudflare Tunnel, peers hold standby tunnels.",
          "04")

    # Cloudflare cloud (top, positioned below title)
    box(38, 79, 24, 8, "Cloudflare Edge", fill=PALETTE["navy"],
        edge=PALETTE["navy"], tcolor=PALETTE["white"], fontsize=9.5,
        weight="bold", sub="Anycast · QUIC tunnels", sub_size=7.4)

    # VPS boxes — providers (2 rows of 3)
    nodes = [
        (10, 52, "Hetzner FSN1", "Node A · LEADER", PALETTE["teal_lt"],
         PALETTE["teal"]),
        (39, 52, "DigitalOcean FRA1", "Node B · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (68, 52, "Vultr FRA", "Node C · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (10, 17, "On-Prem / Homelab", "Node D · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (39, 17, "Hetzner HEL1", "Node E · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
        (68, 17, "AWS eu-central-1", "Node F · Standby", PALETTE["slate_lt"],
         PALETTE["slate"]),
    ]

    # Containers inside each VPS
    for x, y, prov, name, fi, ed in nodes:
        box_(x, y, 22, 18, fill=PALETTE["white"], edge=ed, radius=1.4)
        label(x + 11, y + 15.5, prov, fontsize=8.8, weight="bold",
              color=PALETTE["slate_dk"])
        label(x + 11, y + 13.0, name, fontsize=7.5, color=ed, weight="bold")
        box(x + 1.5, y + 1.8, 9, 9, "WireGuard", fill=fi, edge=ed, fontsize=7.4,
            radius=0.6, sub="L3 mesh", sub_size=6.4)
        box(x + 11.5, y + 1.8, 9, 9, "BRIDGE", fill=fi, edge=ed, fontsize=7.5,
            weight="bold", radius=0.6, sub="daemon", sub_size=6.4)

    # Active tunnel — leader only
    arrow((44, 79), (21, 70), color=PALETTE["navy"], lw=1.8)
    label(26, 76, "active QUIC tunnel", fontsize=7.8, color=PALETTE["navy"],
          weight="bold")

    # Mesh links — clean grid interconnect
    # Horizontal links
    arrow((32, 61), (39, 61), color=PALETTE["slate"], lw=0.9, ls="--")
    arrow((61, 61), (68, 61), color=PALETTE["slate"], lw=0.9, ls="--")
    arrow((32, 26), (39, 26), color=PALETTE["slate"], lw=0.9, ls="--")
    arrow((61, 26), (68, 26), color=PALETTE["slate"], lw=0.9, ls="--")
    # Vertical links
    arrow((21, 52), (21, 35), color=PALETTE["slate"], lw=0.9, ls="--")
    arrow((50, 52), (50, 35), color=PALETTE["slate"], lw=0.9, ls="--")
    arrow((79, 52), (79, 35), color=PALETTE["slate"], lw=0.9, ls="--")

    # Center banner pill
    box_(24, 42.2, 52, 3.6, fill=WHITE, edge=HAIRLINE, radius=0.5)
    label(50, 44, "WireGuard Mesh · SWIM Gossip (Full Peer Interconnect)",
          fontsize=8.0, color=PALETTE["slate_dk"], weight="bold")

    legend([
        (PALETTE["navy"], "Cloudflare Anycast edge"),
        (PALETTE["teal"], "Active leader node"),
        (PALETTE["slate"], "Peer standby node"),
        (PALETTE["slate"], "WireGuard encrypted mesh", "line"),
    ], x=2.5, y=3.6, w=24, h=11.5, title="Deployment legend")


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


