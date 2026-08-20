"""Gantt chart roadmap generator for BRIDGE architecture phases.

Renders a 10-week implementation roadmap aligned with Section 9 of the
BRIDGE specification, maintaining the shared visual language in style.py.
"""

from __future__ import annotations

from pathlib import Path
import matplotlib.patches as mpatches
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch

from style import (
    figure, title, footer, legend, label, box_, save,
    NAVY, TEAL, TEAL_LT, SLATE, SLATE_LT, SLATE_DK,
    INK, MUTED, HAIRLINE, AMBER, GREEN, GREEN_LT, WHITE, GRID, PALETTE
)


def fig08_roadmap_gantt(out):
    fig, ax = figure("Roadmap Gantt", 14.5, 9.6)
    title(fig, "Implementation Roadmap — 10-Week Build Phases",
          "Ordered so each phase produces a runnable, testable deliverable before moving to the next.",
          "08")

    # Time grid coordinates
    grid_left = 38.0
    grid_right = 96.0
    week_w = (grid_right - grid_left) / 10.0  # 5.8 per week
    top_y = 81.5
    row_h = 6.1
    num_phases = 9
    bot_y = top_y - 4.0 - (num_phases * row_h)  # 81.5 - 4.0 - 54.9 = 22.6

    # Draw week column background stripes & headers
    for w in range(10):
        wx = grid_left + w * week_w
        is_even = (w % 2 == 0)
        col_bg = "#F9FBFC" if is_even else WHITE
        rect = mpatches.Rectangle((wx, bot_y), week_w, top_y - bot_y,
                                  facecolor=col_bg, edgecolor="none", zorder=0)
        ax.add_patch(rect)

        # Header cell
        hdr = mpatches.Rectangle((wx, top_y - 4.0), week_w, 4.0,
                                 facecolor="#E2E8EE" if is_even else "#EDF2F6",
                                 edgecolor=HAIRLINE, linewidth=0.6, zorder=2)
        ax.add_patch(hdr)
        ax.text(wx + week_w / 2, top_y - 2.0, f"Week {w+1}",
                fontsize=8.0, weight="bold", color=SLATE_DK,
                ha="center", va="center", zorder=3)

    # Left header box
    left_hdr = mpatches.Rectangle((3.5, top_y - 4.0), grid_left - 3.5, 4.0,
                                  facecolor=NAVY, edgecolor=NAVY,
                                  linewidth=0.6, zorder=2)
    ax.add_patch(left_hdr)
    ax.text(5.0, top_y - 2.0, "Phase & Core Deliverable", fontsize=8.4,
            weight="bold", color=WHITE, ha="left", va="center", zorder=3)

    # Vertical week separator grid lines
    for w in range(11):
        wx = grid_left + w * week_w
        ax.plot([wx, wx], [bot_y, top_y], color=HAIRLINE, lw=0.7, ls="-", zorder=1)

    # Phase data: (id, name, deliverable, start_week, end_week, track_color, fill_color, done_text)
    phases = [
        ("Phase 1", "Core Proxy", "Single-node reverse proxy · TOML routes · rustls ACME",
         1, 2, NAVY, NAVY, "✓ Valid TLS reverse proxy"),
        ("Phase 2", "WireGuard Mesh", "Programmatic peer mesh · wg-control · Tonic gRPC",
         3, 4, TEAL, TEAL, "✓ 2-node tunnel & RPC sync"),
        ("Phase 3", "Membership (SWIM)", "Embedded foca SWIM · failure detection & recovery",
         4, 5, TEAL, TEAL, "✓ Heartbeat TTL probe failover"),
        ("Phase 4", "Routing Gossip", "Routing table digest sync · vector clocks / CRDT",
         6, 6, TEAL, TEAL, "✓ Fleet service propagation"),
        ("Phase 5", "Leader Election", "Bully election · quorum verification (⌈n/2⌉)",
         7, 7, NAVY, NAVY, "✓ Auto-elects on node failure"),
        ("Phase 6", "Handoff (Tunnel)", "Cloudflare Tunnel takeover · warm/cold standby",
         8, 8, NAVY, NAVY, "✓ Sub-5s ingress handoff"),
        ("Phase 7", "Handoff (Floating IP)", "Provider API failover (Hetzner / DigitalOcean)",
         8, 9, SLATE, SLATE, "✓ Direct IP mobility via API"),
        ("Phase 8", "Consistent Hashing", "360 vnodes ring · SHA256 affinity · load spread",
         9, 9, TEAL, TEAL, "✓ Uniform load distribution"),
        ("Phase 9", "CLI, UI & Polish", "bridge CLI status/inspect · web UI · documentation",
         10, 10, GREEN, GREEN, "✓ Full fleet observability"),
    ]

    y_cursor = top_y - 4.0

    for idx, (p_id, p_name, p_deliv, sw, ew, tcolor, fcolor, p_done) in enumerate(phases):
        ry_top = y_cursor - idx * row_h
        ry_bot = ry_top - row_h
        cy = (ry_top + ry_bot) / 2.0

        # Alternating row background for left panel
        if idx % 2 == 0:
            left_bg = mpatches.Rectangle((3.5, ry_bot), grid_left - 3.5, row_h,
                                         facecolor="#F6F8FA", edgecolor="none", zorder=0)
            ax.add_patch(left_bg)

        # Left label: Phase ID + Title + Deliverable
        ax.text(4.5, cy + 1.2, f"{p_id} — {p_name}", fontsize=8.6,
                weight="bold", color=INK, ha="left", va="center", zorder=3)
        ax.text(4.5, cy - 1.3, p_deliv, fontsize=7.0,
                color=MUTED, style="italic", ha="left", va="center", zorder=3)

        # Horizontal row divider line across entire table
        ax.plot([3.5, grid_right], [ry_bot, ry_bot], color=GRID, lw=0.7, zorder=1)

        # Gantt Bar on grid
        bar_x1 = grid_left + (sw - 1) * week_w + 0.4
        bar_x2 = grid_left + ew * week_w - 0.4
        bar_w = bar_x2 - bar_x1
        bar_h = 3.2
        bar_y = cy - bar_h / 2.0

        # Bar patch with rounded corners
        bar_patch = FancyBboxPatch((bar_x1, bar_y), bar_w, bar_h,
                                   boxstyle="round,pad=0.02,rounding_size=0.6",
                                   facecolor=fcolor, edgecolor=tcolor,
                                   linewidth=1.0, zorder=3)
        ax.add_patch(bar_patch)

        # Duration text inside bar
        duration_text = f"W{sw}–W{ew}" if sw != ew else f"W{sw}"
        ax.text(bar_x1 + bar_w / 2.0, cy, duration_text, fontsize=7.6,
                weight="bold", color=WHITE, ha="center", va="center", zorder=4)

        # Milestone done pill / marker (draw to the right if space allows, or to the left)
        avail_right = grid_right - bar_x2
        pill_w = len(p_done) * 0.46 + 2.4
        if avail_right >= (pill_w + 1.2):
            pill_x = bar_x2 + 0.8
            ax.plot([bar_x2, pill_x], [cy, cy], color=SLATE, lw=0.9, ls=":", zorder=2)
            box_(pill_x, cy - 1.25, pill_w, 2.5, fill=WHITE, edge=HAIRLINE,
                 radius=0.4, lw=0.7, zorder=3)
            ax.text(pill_x + pill_w/2.0, cy, p_done, fontsize=6.6,
                    color=SLATE_DK, weight="bold", ha="center", va="center", zorder=4)
        else:
            # Place to the left of the bar
            pill_x = bar_x1 - pill_w - 0.8
            if pill_x > grid_left:
                ax.plot([pill_x + pill_w, bar_x1], [cy, cy], color=SLATE, lw=0.9, ls=":", zorder=2)
                box_(pill_x, cy - 1.25, pill_w, 2.5, fill=WHITE, edge=HAIRLINE,
                     radius=0.4, lw=0.7, zorder=3)
                ax.text(pill_x + pill_w/2.0, cy, p_done, fontsize=6.6,
                        color=SLATE_DK, weight="bold", ha="center", va="center", zorder=4)


    # Outer border of table
    ax.plot([3.5, 3.5], [bot_y, top_y], color=HAIRLINE, lw=0.9, zorder=2)
    ax.plot([grid_right, grid_right], [bot_y, top_y], color=HAIRLINE, lw=0.9, zorder=2)
    ax.plot([3.5, grid_right], [top_y, top_y], color=HAIRLINE, lw=0.9, zorder=2)
    ax.plot([3.5, grid_right], [bot_y, bot_y], color=HAIRLINE, lw=0.9, zorder=2)
    ax.plot([grid_left, grid_left], [bot_y, top_y], color=HAIRLINE, lw=0.9, zorder=2)

    # Bottom summary / legend
    legend([
        (NAVY, "Data Plane & Leader Election"),
        (TEAL, "Mesh Clustering & Gossip"),
        (SLATE, "Provider IP Mobility (Mode 2)"),
        (GREEN, "Tooling & Observability"),
    ], x=3.5, y=3.8, w=32, h=14.0, title="Track Category")

    # Bottom info box
    box_(37.5, 3.8, 58.5, 14.0, fill=WHITE, edge=HAIRLINE, radius=0.6, lw=0.8)
    label(66.75, 15.6, "Sequential Delivery & Verification Principles", fontsize=8.6,
          weight="bold", color=NAVY)
    label(66.75, 13.0, "• Zero Speculative Abstraction: Every phase produces a standalone, functional, runnable binary.",
          fontsize=7.4, color=SLATE_DK, ha="center")
    label(66.75, 10.6, "• Strict Quality Gates: No subsequent phase begins until prior deliverables pass integration testing.",
          fontsize=7.4, color=SLATE_DK, ha="center")
    label(66.75, 8.2, "• Incremental Complexity: Single-node proxy → 2-node mesh → SWIM gossip → Quorum election → Multi-cloud.",
          fontsize=7.4, color=SLATE_DK, ha="center")
    label(66.75, 5.8, "• Target Outcome: Fully autonomous multi-cloud edge mesh with zero single points of failure in 10 weeks.",
          fontsize=7.4, color=TEAL, weight="bold", ha="center")

    footer(fig)
    return save(fig, out)


    footer(fig)
    return save(fig, out)


if __name__ == "__main__":
    out_dir = Path(__file__).parent.parent / "assets"
    out_dir.mkdir(exist_ok=True)
    out_file = str(out_dir / "08_roadmap_gantt.png")
    fig08_roadmap_gantt(out_file)
    print(f"wrote {out_file}")
