"""Style + primitive helpers for BRIDGE architecture diagrams.

Single source of truth for the visual language. Every diagram module
imports from here so the look stays consistent across all artifacts.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, Tuple

import threading

import matplotlib.patches as mpatches
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch

# Track the active axes so helpers don't need an explicit `ax` argument.
_active = threading.local()


def _ax():
    return getattr(_active, "ax", plt.gca())

# --- Palette ---------------------------------------------------------------
# Clean technical look: white bg, navy/teal accents, grayscale network.
NAVY       = "#0B2545"   # primary headers, leaders
TEAL       = "#13678A"   # accents, active state
TEAL_LT    = "#D9E8EC"   # fills for accent boxes
SLATE      = "#45596C"   # secondary boxes
SLATE_LT   = "#E3E8ED"   # secondary fills
SLATE_DK   = "#2C3A45"   # dark text on light fills
INK        = "#1A1F24"   # near-black text
MUTED      = "#6B7A85"   # axis text, captions
HAIRLINE   = "#C5CED4"   # thin separators
AMBER      = "#B8860B"   # warnings / open questions
CRIMSON    = "#A02C2C"   # failures / dead nodes
CRIMSON_LT = "#F5DCDC"
GREEN      = "#1F7A4D"   # success / alive / recovered
GREEN_LT   = "#DCF0E5"
GREY       = "#9BA3AB"   # neutral nodes, disabled
WHITE      = "#FFFFFF"
GRID       = "#EEF1F4"

PALETTE = dict(
    navy=NAVY, teal=TEAL, teal_lt=TEAL_LT, slate=SLATE,
    slate_lt=SLATE_LT, slate_dk=SLATE_DK, ink=INK, muted=MUTED,
    hairline=HAIRLINE, amber=AMBER, crimson=CRIMSON, crimson_lt=CRIMSON_LT,
    green=GREEN, green_lt=GREEN_LT, grey=GREY, white=WHITE, grid=GRID,
)


# --- Figure scaffold -------------------------------------------------------
def figure(name: str, w: float = 12, h: float = 7):
    """Create a figure with the standard BRIDGE canvas."""
    fig = plt.figure(name, figsize=(w, h), dpi=192)
    fig.patch.set_facecolor(WHITE)
    ax = fig.add_axes([0, 0, 1, 1])
    _active.ax = ax
    ax.set_facecolor(WHITE)
    ax.set_xlim(0, 100)
    ax.set_ylim(0, 100)
    ax.set_axis_off()
    return fig, ax


def title(fig, text: str, sub: str | None = None, num: str | None = None):
    """Top-left title block with optional subtitle and artifact number."""
    fig.text(0.04, 0.96, text, fontsize=15, weight="bold", color=NAVY,
             ha="left", va="top")
    if sub:
        fig.text(0.04, 0.92, sub, fontsize=9.5, color=MUTED, ha="left",
                 va="top", style="italic")
    if num:
        fig.text(0.96, 0.96, num, fontsize=9, color=HAIRLINE, ha="right",
                 va="top", family="monospace")


def footer(fig, text: str = "BRIDGE · Personal Architecture Doc · Peniel Ben · v0.4"):
    fig.text(0.04, 0.015, text, fontsize=7.6, color=MUTED, ha="left", va="bottom")
    fig.text(0.96, 0.015, "Living Document", fontsize=7.6, color=MUTED,
             ha="right", va="bottom", style="italic")


# --- Primitive drawing ------------------------------------------------------
def box(x, y, w, h, label, *, fill=SLATE_LT, edge=SLATE, tcolor=INK,
        radius=1.2, lw=1.2, fontsize=10, weight="normal", sub=None,
        halign="center", sub_size=8):
    """Rounded rectangle with centered label. Supports an optional sub-line."""
    ax = _ax()
    p = FancyBboxPatch((x, y), w, h, boxstyle=f"round,pad=0.02,rounding_size={radius}",
                       linewidth=lw, edgecolor=edge, facecolor=fill,
                       mutation_aspect=0.08, zorder=2)
    ax.add_patch(p)
    cy = y + h/2 + (1.4 if sub else 0)
    ax.text(x + w/2, cy, label, ha=halign if halign != "center" else "center",
            va="center", fontsize=fontsize, weight=weight, color=tcolor, zorder=3)
    if sub:
        ax.text(x + w/2, cy - 2.5, sub, ha="center", va="center", fontsize=sub_size,
                color=MUTED, style="italic", zorder=3)
    return p


def diamond(x, y, w, h, label, *, fill=TEAL_LT, edge=TEAL,
            tcolor=SLATE_DK, lw=1.2, fontsize=9.5):
    """Decision diamond."""
    ax = _ax()
    cx, cy = x + w/2, y + h/2
    verts = [(cx, y + h), (x + w, cy), (cx, y), (x, cy)]
    poly = mpatches.Polygon(verts, closed=True, facecolor=fill,
                            edgecolor=edge, linewidth=lw, zorder=2)
    ax.add_patch(poly)
    ax.text(cx, cy, label, ha="center", va="center", fontsize=fontsize,
            color=tcolor, weight="bold", zorder=3)
    return poly


def circle(x, y, d, label, *, fill=TEAL_LT, edge=TEAL, tcolor=SLATE_DK,
           lw=1.2, fontsize=9, sub=None, sub_size=7.5):
    """Circular node. (x, y) is top-left of the bounding box of the circle."""
    ax = _ax()
    c = mpatches.Circle((x + d/2, y + d/2), d/2, facecolor=fill,
                        edgecolor=edge, linewidth=lw, zorder=2)
    ax.add_patch(c)
    cy = y + d/2 + (0.8 if sub else 0)
    ax.text(x + d/2, cy, label, ha="center", va="center", fontsize=fontsize,
            color=tcolor, weight="bold", zorder=3)
    if sub:
        ax.text(x + d/2, cy - 2.2, sub, ha="center", va="center", fontsize=sub_size,
                color=MUTED, style="italic", zorder=3)
    return c


def arrow(p1, p2, *, color=SLATE_DK, lw=1.3, ls="-", style="-|>",
          shrink=0.0, rad=0.0, zorder=1, alpha=1.0):
    """Connector arrow between two points."""
    ax = _ax()
    a = FancyArrowPatch(p1, p2, arrowstyle=style, color=color, lw=lw,
                       linestyle=ls, shrinkA=shrink, shrinkB=shrink,
                       connectionstyle=f"arc3,rad={rad}", zorder=zorder,
                       alpha=alpha, mutation_scale=12)
    ax.add_patch(a)
    return a


def label(x, y, text, *, color=MUTED, fontsize=8.5, ha="center", va="center",
          weight="normal", style="italic", rotation=0):
    """Free-floating text label."""
    _ax().text(x, y, text, ha=ha, va=va, fontsize=fontsize, color=color,
               weight=weight, style=style, rotation=rotation, zorder=4)


def legend(items=None, x: float = 3, y: float = 4.2, w: float = 24, h: float = 11.2,
           title: str | None = None, ax=None, **kwargs):
    """Soft legend box. items: list of (color, label, [optional line_style])."""
    if items is not None and not isinstance(items, (list, tuple)):
        # Support legacy calling convention where ax was passed as first arg
        items = kwargs.get("items", None)
    if items is None:
        items = kwargs.get("items", [])

    ax_active = _ax()
    box_(x, y, w, h, fill=WHITE, edge=HAIRLINE, radius=0.6, lw=0.8)
    if title:
        ax_active.text(x + w/2, y + h - 1.6, title, fontsize=8.2, color=SLATE_DK,
                       weight="bold", ha="center", va="top", zorder=3)
    n = max(len(items), 1)
    top_offset = 3.5 if title else 1.6
    available_h = h - top_offset - 1.0
    spacing = available_h / max(n - 1, 1) if n > 1 else 0
    ly = y + h - top_offset
    for item in items:
        color = item[0]
        lab = item[1]
        is_line = len(item) > 2 and item[2] == "line"
        if is_line:
            ax_active.plot([x + 1.8, x + 3.8], [ly, ly], color=color, lw=1.2, ls="--", zorder=3)
        else:
            sw = mpatches.Rectangle((x + 1.8, ly - 0.6), 1.6, 1.2, facecolor=color,
                                    edgecolor=HAIRLINE if color == WHITE else "none",
                                    linewidth=0.5, zorder=3)
            ax_active.add_patch(sw)
        ax_active.text(x + 4.2, ly, lab, fontsize=7.2, color=SLATE_DK,
                       ha="left", va="center", zorder=3)
        ly -= spacing




def box_(x, y, w, h, *, fill=WHITE, edge=HAIRLINE, radius=1.0, lw=0.8,
        zorder=1):
    """Internal: plain rounded rect without text."""
    p = FancyBboxPatch((x, y), w, h, boxstyle=f"round,pad=0.02,rounding_size={radius}",
                       linewidth=lw, edgecolor=edge, facecolor=fill, zorder=zorder)
    _ax().add_patch(p)
    return p


def save(fig, path: str):
    """Save with consistent margins and dpi. Returns the path."""
    fig.savefig(path, dpi=192, facecolor=WHITE, bbox_inches="tight",
                pad_inches=0.15)
    plt.close(fig)
    return path

