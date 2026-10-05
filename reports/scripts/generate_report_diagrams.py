#!/usr/bin/env python3
"""
Generate publication-quality architectural and benchmark figures for SIWES Technical Report.
Design system aligns strictly with a simple, classy, clean two-color academic/technical style:
- Clean lines
- Restrained typography
- Balanced whitespace (margins at 4% and 96%, balanced left/right visual weight)
- Minimal decoration (no unnecessary gradients, shadows, or glow)
- Consistent visual language across the report
"""

import sys
import os
from pathlib import Path

# Add docs/scripts to path to import style
ROOT_DIR = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT_DIR / "docs" / "scripts"))
import matplotlib.pyplot as plt
import matplotlib.patches as mp
import numpy as np

from style import (
    arrow, box, box_, figure, footer, label, legend, PALETTE, save,
    title, circle, WHITE, HAIRLINE
)

OUT_DIR = ROOT_DIR / "report_figures"
OUT_DIR.mkdir(exist_ok=True)

# Restrained academic palette: Deep Navy, Infrastructure Teal/Slate, Clean Backgrounds
NAVY = "#0B2545"
TEAL = "#13678A"
SLATE = "#45596C"
LIGHT_BG = "#F4F7FA"
ACCENT_BG = "#E8EEF3"
WHITE = "#FFFFFF"
BORDER = "#A5B4C2"


# ==============================================================================
# Figure 2.1: AKIRS Directorate & ICT Infrastructure Hierarchy
# ==============================================================================
def make_fig2_1():
    fig, ax = figure("AKIRS Org Hierarchy", 13.5, 9.0)
    title(fig, "AKIRS Directorate & Technology Systems Hierarchy",
          "Governance and operational structure of the Akwa Ibom State Internal Revenue Service.", "2.1")

    # Executive Leadership - perfectly centered at x=50
    box(32.0, 75.0, 36.0, 8.5, "Executive Chairman & Board of Internal Revenue",
        fill=NAVY, edge=NAVY, tcolor=WHITE,
        fontsize=9.2, weight="bold", sub="Statutory fiscal leadership · Policy & strategic oversight", sub_size=7.0)

    # Vertical stem from x=50
    arrow((50.0, 75.0), (50.0, 67.0), color=NAVY, lw=1.5)
    # Horizontal distribution rail spanning x=14.75 to 85.25
    ax.plot([14.75, 85.25], [67.0, 67.0], color=NAVY, lw=1.5)

    # 4 Main Directorates - width 21.5 each, spacing 2.0, total width = 92 (x=4 to 96)
    directorates = [
        (4.0, 38.0, 21.5, 26.0, "Directorate of ICT\n(Host Engineering Unit)",
         "• Infrastructure & Networks\n• Software Engineering\n• Database Administration\n• Cyber & Endpoint Security\n• Subnational Datacenter Node",
         True),
        (27.5, 38.0, 21.5, 26.0, "Directorate of Tax Intelligence,\nResearch & Enforcement",
         "• OSINT Taxpayer Recon\n• Third-Party Data Linkage\n• Commercial Entity Mapping\n• Field Enforcement Ops\n• Direct Assessment Leads",
         False),
        (51.0, 38.0, 21.5, 26.0, "Assessment & Collection\nDirectorates",
         "• Direct Assessment\n• PAYE (Corporate Employers)\n• Withholding Tax (WHT)\n• Capital Gains & Stamp Duty\n• Motor Vehicle Licensing",
         False),
        (74.5, 38.0, 21.5, 26.0, "Corporate Services &\nLegal Affairs",
         "• Revenue Law Enforcement\n• Compliance & Dispute Resol.\n• Finance & Accounts\n• Human Resource Mgmt\n• Internal Audit & Control",
         False),
    ]

    for x, y, w, h, name, bullets, is_host in directorates:
        # Arrow from rail down to directorate top
        arrow((x + w/2, 67.0), (x + w/2, y + h), color=NAVY, lw=1.3)
        bg = ACCENT_BG if is_host else LIGHT_BG
        edge = TEAL if is_host else SLATE
        box_(x, y, w, h, fill=bg, edge=edge, radius=0.8, lw=1.3 if is_host else 1.0)
        label(x + w/2, y + h - 3.2, name, fontsize=8.2, weight="bold", color=NAVY)
        ax.plot([x + 1.5, x + w - 1.5], [y + h - 6.5, y + h - 6.5], color=BORDER, lw=0.6)
        ax.text(x + 2.0, y + h - 8.2, bullets, fontsize=6.8, color=NAVY,
                va="top", ha="left", linespacing=1.35)

    # Bottom Foundation: Cross-Cutting Revenue Platforms spanning x=4 to 96
    box_(4.0, 7.5, 92.0, 24.5, fill=LIGHT_BG, edge=NAVY, radius=1.0, lw=1.4)
    label(50.0, 28.5, "CORE REVENUE DIGITAL INFRASTRUCTURE & SYSTEMS (CROSS-CUTTING PLATFORMS)",
          fontsize=9.2, weight="bold", color=NAVY)

    platforms = [
        (4.0 + 2.0, 10.5, 20.5, 14.5, "IbomTax Portal", "Enterprise Web Ingress\nTaxpayer self-service & e-filing\nAutomated receipting & assessment"),
        (27.5 + 0.5, 10.5, 20.5, 14.5, "AISTIN Registry", "State Taxpayer ID Registry\nBiometric & BVN resolution\nUnified taxpayer master file"),
        (51.0 - 1.0, 10.5, 20.5, 14.5, "Data Cleaner & ETL", "FastAPI bank data cleaner\nNUBAN mod-11 & TIN check\nIdentity deduplication engine"),
        (74.5 - 2.5, 10.5, 21.0, 14.5, "Intelligence Recon", "Playwright Meta ads OSINT\nCorporate recon pipeline\nSpatial LGA business targeting"),
    ]
    for px, py, pw, ph, pname, pdesc in platforms:
        box_(px, py, pw, ph, fill=WHITE, edge=TEAL, radius=0.6, lw=1.0)
        label(px + pw/2, py + ph - 2.8, pname, fontsize=8.0, weight="bold", color=NAVY)
        ax.text(px + pw/2, py + ph - 5.5, pdesc, fontsize=6.6, color=SLATE,
                ha="center", va="top", linespacing=1.25)

    # Balanced connecting arrows from directorates to shared platforms
    for cx in [14.75, 38.25, 61.75, 85.25]:
        arrow((cx, 38.0), (cx, 32.0), color=NAVY, lw=1.1)

    footer(fig, "SIWES Technical Report · Academic Architecture Series · Peniel Ben")
    out_path = OUT_DIR / "fig2_1_akirs_hierarchy.png"
    save(fig, str(out_path))
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 2.2: JRB Subnational Tax Modernization Architecture
# ==============================================================================
def make_fig2_2():
    fig, ax = figure("JRB Subnational Architecture", 13.5, 9.0)
    title(fig, "Joint Revenue Board (JRB) Subnational Tax Modernization Architecture",
          "Inter-agency national-to-state infrastructure topology and regional datacenter node.", "2.2")

    # Top: Federal & National Clearing Layer (x=4 to 96)
    box_(4.0, 68.0, 92.0, 20.0, fill=LIGHT_BG, edge=SLATE, radius=1.0, lw=1.3)
    label(50.0, 84.5, "NATIONAL REVENUE HARMONIZATION & INTER-BANK CLEARING LAYER",
          fontsize=9.2, weight="bold", color=NAVY)

    nat_nodes = [
        (6.0, 70.0, 20.5, 11.0, "Joint Tax Board (JTB / JRB)", "National TIN Registry\nTax Harmonization Hub"),
        (29.0, 70.0, 20.5, 11.0, "NIBSS Clearing House", "Inter-Bank Settlement Feeds\nBVN Verification Gateway"),
        (52.0, 70.0, 20.5, 11.0, "CAC Corporate Registry", "Entity Incorporation Records\nDirectors & Shareholder Data"),
        (75.0, 70.0, 19.5, 11.0, "Commercial Banks", "Monthly Bulk Account Feeds\nDirect Tax Deduction Returns"),
    ]
    for x, y, w, h, name, desc in nat_nodes:
        box_(x, y, w, h, fill=WHITE, edge=NAVY, radius=0.6, lw=1.0)
        label(x + w/2, y + h - 2.8, name, fontsize=7.8, weight="bold", color=NAVY)
        ax.text(x + w/2, y + h - 5.5, desc, fontsize=6.6, color=SLATE,
                ha="center", va="top", linespacing=1.2)

    # Secure Communications Gateway
    for cx in [16.25, 39.25, 62.25, 84.75]:
        arrow((cx, 68.0), (cx, 57.0), color=NAVY, lw=1.2)

    box(20.0, 52.0, 60.0, 6.5, "Secure Encrypted GovNet / Dedicated IPsec VPN Gateway",
        fill=ACCENT_BG, edge=TEAL, fontsize=8.4, weight="bold",
        sub="TLS 1.3 / AES-256-GCM Encrypted Inter-Agency Transmission Channel", sub_size=6.8)

    arrow((50.0, 52.0), (50.0, 44.0), color=NAVY, lw=1.5)

    # Main Body: AKIRS Subnational Datacenter Node (x=4 to 96)
    box_(4.0, 6.0, 92.0, 38.0, fill=LIGHT_BG, edge=NAVY, radius=1.0, lw=1.5)
    label(50.0, 40.5, "AKIRS REVENUE HOUSE DATACENTER NODE (UYO, AKWA IBOM STATE)",
          fontsize=9.8, weight="bold", color=NAVY)

    # 3 Balanced Pillars inside Datacenter
    dc_pillars = [
        (6.5, 9.0, 27.5, 28.0, "Perimeter Security & Ingress",
         "• Sophos XGS 138 Next-Gen Firewall\n• Dual WAN: Fiber ISP + Starlink HA\n• Cisco Catalyst 1300 Core Switch\n• VLAN DMZ & Network Isolation\n• Perimeter Ingress Bastion Routing"),
        (36.25, 9.0, 27.5, 28.0, "Virtual Compute Cluster",
         "• Dual HPE ProLiant DL380 Gen11\n• VMware ESXi 8.0 Hypervisors\n• VMware vCenter Server VCSA\n• DRS Automated Load Balancing\n• vMotion Zero-Downtime Migration"),
        (66.0, 9.0, 27.5, 28.0, "Storage & Data Protection",
         "• Synology DSM Enterprise NAS\n• Hardware RAID 5 Fault Tolerance\n• Veeam Backup & Replication\n• Immutable VM Snapshots\n• Off-Site Encrypted Replication"),
    ]
    for x, y, w, h, name, bullets in dc_pillars:
        box_(x, y, w, h, fill=WHITE, edge=TEAL, radius=0.8, lw=1.1)
        label(x + w/2, y + h - 3.0, name, fontsize=8.2, weight="bold", color=NAVY)
        ax.plot([x + 1.5, x + w - 1.5], [y + h - 6.0, y + h - 6.0], color=BORDER, lw=0.6)
        ax.text(x + 2.0, y + h - 8.0, bullets, fontsize=7.0, color=NAVY,
                va="top", ha="left", linespacing=1.35)

    footer(fig, "SIWES Technical Report · Academic Architecture Series · Peniel Ben")
    out_path = OUT_DIR / "fig2_2_jrb_modernization.png"
    save(fig, str(out_path))
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 3.1: AKIRS Datacenter Network Architecture
# ==============================================================================
def make_fig3_1():
    fig, ax = figure("AKIRS Datacenter Network", 13.5, 9.2)
    title(fig, "AKIRS Enterprise Datacenter Infrastructure Topology",
          "High-availability dual-WAN ingress, VLAN segmentation, compute virtualization, and storage.", "3.1")

    # Layer 1: Dual WAN (Balanced at x=15 to 45 and x=55 to 85)
    box(15.0, 77.0, 32.0, 8.5, "Primary ISP Link (Terrestrial Fiber)", fill=LIGHT_BG,
        edge=SLATE, fontsize=8.0, weight="bold", sub="100 Mbps Symmetrical Metro Fiber Uplink", sub_size=6.6)
    box(53.0, 77.0, 32.0, 8.5, "Secondary Backup (Starlink Satellite)", fill=ACCENT_BG,
        edge=TEAL, fontsize=8.0, weight="bold", sub="Low-Earth Orbit High-Bandwidth Backup", sub_size=6.6)

    arrow((31.0, 77.0), (44.0, 68.0), color=NAVY, lw=1.3)
    arrow((69.0, 77.0), (56.0, 68.0), color=NAVY, lw=1.3)

    # Layer 2: Firewall (Centered at x=50)
    box(30.0, 59.0, 40.0, 9.0, "Sophos XGS 138 Next-Generation Firewall", fill=NAVY,
        edge=NAVY, tcolor=WHITE, fontsize=8.8, weight="bold",
        sub="Dual-WAN Active-Backup Failover · Stateful IPS/IDS · NAT Gateway", sub_size=6.8)

    arrow((50.0, 59.0), (50.0, 50.0), color=NAVY, lw=1.5)

    # Layer 3: Cisco Core Switch (Centered at x=50, w=60)
    box(20.0, 43.0, 60.0, 7.0, "Cisco Catalyst 1300 Core Managed Switch (802.1Q Trunking)",
        fill=ACCENT_BG, edge=TEAL, fontsize=8.8, weight="bold",
        sub="Layer 2/3 Hardware Forwarding · Port Isolation · 10Gbps SFP+ Uplinks", sub_size=6.8)

    # Layer 4: 4 VLAN breakout rails (x=4 to 96, w=21.5 each)
    vlans = [
        (4.0, 11.0, 21.5, 24.0, "VLAN 10: Management",
         "• Subnet: 10.10.10.0/24\n• ESXi Host Management\n• vCenter Server VCSA\n• Synology DSM Admin UI\n• PDU & UPS Controllers"),
        (27.5, 11.0, 21.5, 24.0, "VLAN 20: DMZ / Ingress",
         "• Subnet: 10.10.20.0/24\n• Ingress Bastion Gateway\n• Reverse Proxy Nodes\n• Public SSL Termination\n• Air-Gapped Firewall Rules"),
        (51.0, 11.0, 21.5, 24.0, "VLAN 30: App & DB Cluster",
         "• Subnet: 10.10.30.0/24\n• IbomTax Production VMs\n• PostgreSQL / MySQL DBs\n• Data Cleaner Backend\n• Intelligence Recon API"),
        (74.5, 11.0, 21.5, 24.0, "VLAN 40: Backup & Storage",
         "• Subnet: 10.10.40.0/24\n• Synology NAS (RAID 5)\n• Veeam Backup Appliance\n• iSCSI Storage Target\n• VM Snapshot Repository"),
    ]

    for x, y, w, h, name, bullets in vlans:
        arrow((x + w/2, 43.0), (x + w/2, y + h), color=NAVY, lw=1.2)
        box_(x, y, w, h, fill=LIGHT_BG, edge=NAVY, radius=0.8, lw=1.1)
        label(x + w/2, y + h - 2.8, name, fontsize=8.0, weight="bold", color=NAVY)
        ax.plot([x + 1.5, x + w - 1.5], [y + h - 5.5, y + h - 5.5], color=BORDER, lw=0.6)
        ax.text(x + 2.0, y + h - 7.2, bullets, fontsize=6.8, color=NAVY,
                va="top", ha="left", linespacing=1.35)

    footer(fig, "SIWES Technical Report · Academic Architecture Series · Peniel Ben")
    out_path = OUT_DIR / "fig3_1_datacenter_network.png"
    save(fig, str(out_path))
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 3.2: Automated Taxpayer Data Cleaning Pipeline (akirs-data-cleaner)
# ==============================================================================
def make_fig3_2():
    fig, ax = figure("Data Cleaner Pipeline", 13.5, 9.2)
    title(fig, "Automated Taxpayer Data Cleaning & Deduplication Pipeline (akirs)",
          "Algorithmic flow from raw commercial bank spreadsheets to consolidated tax database.", "3.2")

    # 4 Pipeline Stages (x=4 to 96, w=21.5 each)
    stages = [
        (4.0, 52.0, 21.5, 30.0, "1. INGESTION LAYER",
         "• FastAPI Upload Endpoint\n• Multi-Sheet Excel Parser\n  (openpyxl memory reader)\n• CSV BOM & Encoding\n• Streaming Row Extractor\n• Raw Input Audit Store"),
        (27.5, 52.0, 21.5, 30.0, "2. HEURISTIC MAPPING",
         "• SYNONYMS Dictionary\n• Automatic Header Lookup:\n  - TAXPAYER_ID / TIN\n  - NUBAN / ACCT_NO\n  - BVN / CUST_ID\n  - PHONE / TEL\n  - NAME / BUSINESS\n• Unmapped Column Flag"),
        (51.0, 52.0, 21.5, 30.0, "3. ALGORITHMIC CHECK",
         "• CBN NUBAN Modulo-11:\n  A = [BankCode || Acct]\n  W = [3,7,3,3,7,3,3,7,3,3,7,3]\n  c = (10 - (∑ A_i*W_i % 10))\n• BVN 11-Digit Length\n• Branch Scope Filtering\n• Quarantine Flagging"),
        (74.5, 52.0, 21.5, 30.0, "4. IDENTITY RESOLUTION",
         "• Primary Key Grouping:\n  Group by NUBAN & BVN\n• Fuzzy String Matching:\n  Jaro-Winkler Metric\n  Levenshtein Distance\n• Conflict Group Builder:\n  Drop, Merge, or Select"),
    ]

    for x, y, w, h, name, bullets in stages:
        box_(x, y, w, h, fill=LIGHT_BG, edge=NAVY, radius=0.8, lw=1.1)
        label(x + w/2, y + h - 2.8, name, fontsize=8.2, weight="bold", color=NAVY)
        ax.plot([x + 1.5, x + w - 1.5], [y + h - 5.5, y + h - 5.5], color=BORDER, lw=0.6)
        ax.text(x + 2.0, y + h - 7.5, bullets, fontsize=6.8, color=NAVY,
                va="top", ha="left", linespacing=1.35)

    # Linear inter-stage arrows
    arrow((25.5, 67.0), (27.5, 67.0), color=NAVY, lw=1.4)
    arrow((49.0, 67.0), (51.0, 67.0), color=NAVY, lw=1.4)
    arrow((72.5, 67.0), (74.5, 67.0), color=NAVY, lw=1.4)

    # Clean transition arrow from Stage 4 down to Output Layer
    arrow((85.25, 52.0), (85.25, 42.0), color=NAVY, lw=1.4)
    arrow((85.25, 42.0), (50.0, 42.0), color=NAVY, lw=1.4)
    arrow((50.0, 42.0), (50.0, 36.0), color=NAVY, lw=1.4)

    # Bottom: Output & Reconciliation (x=4 to 96)
    box_(4.0, 7.5, 92.0, 28.5, fill=WHITE, edge=NAVY, radius=1.0, lw=1.4)
    label(50.0, 32.5, "5. CONSOLIDATION, EXPORT & INTELLIGENCE RECONCILIATION",
          fontsize=9.4, weight="bold", color=NAVY)

    outputs = [
        (6.5, 10.5, 27.5, 17.0, "Cleaned Consolidate CSV", "Consolidated normalized dataset\nStandardized column headers\nStrictly verified NUBANs & TINs"),
        (36.25, 10.5, 27.5, 17.0, "Quarantine / Exception Log", "Invalid NUBANs / missing PKs\nBranch exclusion records\nAudit trail with source row numbers"),
        (66.0, 10.5, 27.5, 17.0, "Tax Database Ingestion", "AISTIN Master Registry staging\nDirect assessment lead pipeline\nAutomated monthly ETL batch feeds"),
    ]
    for ox, oy, ow, oh, oname, odesc in outputs:
        box_(ox, oy, ow, oh, fill=ACCENT_BG, edge=TEAL, radius=0.6, lw=1.0)
        label(ox + ow/2, oy + oh - 2.8, oname, fontsize=8.0, weight="bold", color=NAVY)
        ax.text(ox + ow/2, oy + oh - 5.8, odesc, fontsize=6.8, color=SLATE,
                ha="center", va="top", linespacing=1.25)

    footer(fig, "SIWES Technical Report · Academic Architecture Series · Peniel Ben")
    out_path = OUT_DIR / "fig3_2_data_cleaner_pipeline.png"
    save(fig, str(out_path))
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 3.3: Intelligent Reconnaissance Engine (akirs-auto)
# ==============================================================================
def make_fig3_3():
    fig, ax = figure("Recon Engine Pipeline", 13.5, 9.2)
    title(fig, "Intelligent OSINT Reconnaissance & Scraping Engine (akirs-auto)",
          "Automated discovery of unregistered commercial advertisers in Akwa Ibom State.", "3.3")

    # Top: Dispatch & Task Orchestration (x=4 to 96)
    box(4.0, 76.0, 26.0, 8.5, "Task Scheduler / CLI", fill=LIGHT_BG, edge=SLATE,
        fontsize=8.0, weight="bold", sub="Target LGA & Industry parameters", sub_size=6.6)
    arrow((30.0, 80.25), (38.0, 80.25), color=NAVY, lw=1.3)

    box(38.0, 76.0, 26.0, 8.5, "Redis Task Queue", fill=ACCENT_BG, edge=TEAL,
        fontsize=8.0, weight="bold", sub="Distributed message broker", sub_size=6.6)
    arrow((64.0, 80.25), (72.0, 80.25), color=NAVY, lw=1.3)

    box(72.0, 76.0, 24.0, 8.5, "Worker Pool", fill=NAVY, edge=NAVY, tcolor=WHITE,
        fontsize=8.0, weight="bold", sub="Asyncio Chromium workers", sub_size=6.6)

    arrow((84.0, 76.0), (84.0, 68.0), color=NAVY, lw=1.4)

    # Middle: Phase 1 & Phase 2 (x=4 to 96)
    # Phase 1: Playwright Browser Automation (w=44, x=4 to 48)
    box_(4.0, 39.0, 44.0, 29.0, fill=LIGHT_BG, edge=NAVY, radius=0.8, lw=1.2)
    label(26.0, 64.5, "PHASE 1: PLAYWRIGHT BROWSER AUTOMATION", fontsize=8.4, weight="bold", color=NAVY)
    ax.text(6.0, 60.5, "• Meta Facebook Ads Library Scraper\n• Headless Chromium via Playwright\n• Dialect & Landmark Keyword Expansion:\n  - Akwa Ibom LGAs (Uyo, Eket, Ikot Ekpene)\n  - Local idioms & high-intent commercial terms\n• Advertiser Ad Profile Extraction:\n  - Business Page Name & Social Handle\n  - Ad Creative, Dates & Spend Range",
            fontsize=6.8, color=NAVY, va="top", ha="left", linespacing=1.35)

    arrow((48.0, 53.5), (52.0, 53.5), color=NAVY, lw=1.5)

    # Phase 2: Tiered Reconnaissance (w=44, x=52 to 96)
    box_(52.0, 39.0, 44.0, 29.0, fill=LIGHT_BG, edge=NAVY, radius=0.8, lw=1.2)
    label(74.0, 64.5, "PHASE 2: TIERED RECONNAISSANCE PIPELINE", fontsize=8.4, weight="bold", color=NAVY)

    recons = [
        (54.0, 41.0, 12.5, 18.0, "Tier 1: Web", "Corporate site\nEmail/phone regex\nPhysical address\n(Early stop)"),
        (68.0, 41.0, 12.5, 18.0, "Tier 2: Maps", "DuckDuckGo\nOpenStreetMap\nNominatim geo\n(Free geospatial)"),
        (82.0, 41.0, 12.5, 18.0, "Tier 3: APIs", "Hunter.io verify\nTomTom Places\nApollo.io enrich\n(Graceful drop)"),
    ]
    for rx, ry, rw, rh, rname, rdesc in recons:
        box_(rx, ry, rw, rh, fill=WHITE, edge=TEAL, radius=0.6, lw=0.9)
        label(rx + rw/2, ry + rh - 2.5, rname, fontsize=7.4, weight="bold", color=NAVY)
        ax.text(rx + rw/2, ry + rh - 5.0, rdesc, fontsize=6.2, color=SLATE,
                ha="center", va="top", linespacing=1.2)

    # Downward transition to Phase 3
    arrow((74.0, 39.0), (74.0, 31.0), color=NAVY, lw=1.4)

    # Bottom: Phase 3: Normalization & Storage (x=4 to 96)
    box_(4.0, 7.5, 92.0, 23.5, fill=WHITE, edge=NAVY, radius=1.0, lw=1.4)
    label(50.0, 27.5, "PHASE 3: 1:1 CONSOLIDATION MATRIX & FISCAL DATA PERSISTENCE",
          fontsize=9.2, weight="bold", color=NAVY)

    p3_units = [
        (6.5, 10.0, 27.5, 14.0, "Relational Normalization", "1:1 Schema (Advertiser × Social URL)\nEliminates CSV multi-value pollution\nEnforced by SQLAlchemy ORM"),
        (36.25, 10.0, 27.5, 14.0, "SQLite / PostgreSQL DB", "Alembic schema migrations\nIndexed by Business Name & Phone\nHistorical campaign tracking"),
        (66.0, 10.0, 27.5, 14.0, "Tax Intelligence Export", "Enriched CSV for Field Enforcement\nDirect Assessment discovery leads\nAutomated tax bracket estimation"),
    ]
    for px, py, pw, ph, pname, pdesc in p3_units:
        box_(px, py, pw, ph, fill=ACCENT_BG, edge=TEAL, radius=0.6, lw=0.9)
        label(px + pw/2, py + ph - 2.5, pname, fontsize=7.8, weight="bold", color=NAVY)
        ax.text(px + pw/2, py + ph - 5.0, pdesc, fontsize=6.6, color=SLATE,
                ha="center", va="top", linespacing=1.2)

    footer(fig, "SIWES Technical Report · Academic Architecture Series · Peniel Ben")
    out_path = OUT_DIR / "fig3_3_recon_engine_pipeline.png"
    save(fig, str(out_path))
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 3.4: AI Tax Assistant & Guardrail Architecture
# ==============================================================================
def make_fig3_4():
    fig, ax = figure("Tax Assistant Guardrails", 13.5, 9.0)
    title(fig, "AI Tax Knowledge Assistant — Guardrail Engineering Architecture",
          "Semantic boundary enforcement and policy grounding for statutory tax queries.", "3.4")

    # 3 Equal Columns (w=28 each, spacing 4.0, spanning x=4 to 96)
    # Col 1: Ingress & Guardrails (x=4 to 32)
    box_(4.0, 14.0, 28.0, 68.0, fill=LIGHT_BG, edge=NAVY, radius=1.0, lw=1.3)
    label(18.0, 78.5, "1. INGRESS & GUARDRAIL FILTER", fontsize=8.4, weight="bold", color=NAVY)

    box(6.0, 62.0, 24.0, 12.0, "Taxpayer Ingress\n(Web / WhatsApp)", fill=WHITE, edge=TEAL,
        fontsize=8.0, weight="bold", sub="Natural language inquiry", sub_size=6.6)

    arrow((18.0, 62.0), (18.0, 54.0), color=NAVY, lw=1.3)

    box_(6.0, 18.0, 24.0, 36.0, fill=ACCENT_BG, edge=TEAL, radius=0.8, lw=1.0)
    label(18.0, 50.5, "Semantic Boundary Classifier", fontsize=7.8, weight="bold", color=NAVY)
    ax.text(7.5, 47.0, "• Anti-Jailbreak Scanner\n• Domain Scope Check:\n  - Tax inquiry → PROCEED\n  - Non-tax query → REJECT\n• PII Redaction & Sanitization\n• Prompt Injection Neutralizer\n• Hardened Rejection Handler",
            fontsize=6.8, color=NAVY, va="top", ha="left", linespacing=1.35)

    # Inter-column arrow 1 -> 2
    arrow((32.0, 36.0), (36.0, 36.0), color=NAVY, lw=1.5)

    # Col 2: Statutory Knowledge Base (x=36 to 64)
    box_(36.0, 14.0, 28.0, 68.0, fill=LIGHT_BG, edge=NAVY, radius=1.0, lw=1.3)
    label(50.0, 78.5, "2. STATUTORY KNOWLEDGE RAG", fontsize=8.4, weight="bold", color=NAVY)

    box(38.0, 62.0, 24.0, 12.0, "AKIRS Knowledge Store\n& Legal Corpus", fill=WHITE, edge=TEAL,
        fontsize=8.0, weight="bold", sub="Statutory revenue provisions", sub_size=6.6)

    arrow((50.0, 62.0), (50.0, 54.0), color=NAVY, lw=1.3)

    box_(38.0, 18.0, 24.0, 36.0, fill=ACCENT_BG, edge=TEAL, radius=0.8, lw=1.0)
    label(50.0, 50.5, "Hybrid Retrieval Pipeline", fontsize=7.8, weight="bold", color=NAVY)
    ax.text(39.5, 47.0, "• Akwa Ibom Law (AKSRAL)\n• Personal Income Tax (PITA)\n• Stamp Duties & Finance Acts\n• AISTIN Registration Rules\n• Tax Clearance (TCC) Code\n• Dense Vector Embeddings\n• Hybrid Keyword Reciprocal Rank",
            fontsize=6.8, color=NAVY, va="top", ha="left", linespacing=1.3)

    # Inter-column arrow 2 -> 3
    arrow((64.0, 36.0), (68.0, 36.0), color=NAVY, lw=1.5)

    # Col 3: LLM Inference & Verification (x=68 to 96)
    box_(68.0, 14.0, 28.0, 68.0, fill=LIGHT_BG, edge=NAVY, radius=1.0, lw=1.3)
    label(82.0, 78.5, "3. INFERENCE & CITATION ENGINE", fontsize=8.4, weight="bold", color=NAVY)

    box_(70.0, 42.0, 24.0, 32.0, fill=ACCENT_BG, edge=TEAL, radius=0.8, lw=1.0)
    label(82.0, 70.5, "Constrained Generation", fontsize=7.8, weight="bold", color=NAVY)
    ax.text(71.5, 67.0, "• Strict Grounding on Corpus\n• Temp = 0.1 for Determinism\n• Zero Speculative Answers\n• Formal Section Quotations\n• Statutory Disclaimer Stamp",
            fontsize=6.8, color=NAVY, va="top", ha="left", linespacing=1.35)

    arrow((82.0, 42.0), (82.0, 34.0), color=NAVY, lw=1.3)

    box(70.0, 18.0, 24.0, 16.0, "Verified Tax Advisory\n& Citation Output", fill=WHITE, edge=NAVY,
        fontsize=8.0, weight="bold", sub="Audited statutory response", sub_size=6.6)

    footer(fig, "SIWES Technical Report · Academic Architecture Series · Peniel Ben")
    out_path = OUT_DIR / "fig3_4_ai_assistant_guardrails.png"
    save(fig, str(out_path))
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 4.8: Gossip Convergence Benchmark Chart
# ==============================================================================
def make_fig4_8():
    fig, ax = plt.subplots(figsize=(10, 5.5), dpi=200)
    fig.patch.set_facecolor(WHITE)
    ax.set_facecolor(WHITE)

    fleet_sizes = [3, 5, 10, 15, 20, 25]
    p95_latencies = [850, 1420, 1480, 1710, 1840, 1920]
    mean_latencies = [620, 1080, 1190, 1380, 1490, 1560]
    rounds = [2, 3, 3, 4, 4, 4]

    # Bar chart for P95 latency
    x = np.arange(len(fleet_sizes))
    width = 0.35

    rects1 = ax.bar(x - width/2, p95_latencies, width, label='95th Percentile Latency (ms)',
                    color=NAVY, edgecolor=NAVY, alpha=0.95)
    rects2 = ax.bar(x + width/2, mean_latencies, width, label='Mean Latency (ms)',
                    color=TEAL, edgecolor=TEAL, alpha=0.85)

    # Labels and annotations
    ax.set_xlabel('Fleet Size (Number of Cluster Nodes)', fontsize=10.5, fontweight='bold', color=NAVY)
    ax.set_ylabel('Convergence Duration (Milliseconds)', fontsize=10.5, fontweight='bold', color=NAVY)
    ax.set_title('Figure 4.8: Gossip Routing Convergence Latency vs Cluster Fleet Size',
                 fontsize=12.0, fontweight='bold', color=NAVY, pad=12)
    ax.set_xticks(x)
    ax.set_xticklabels([f'{n} Nodes\n({r} rounds)' for n, r in zip(fleet_sizes, rounds)], fontsize=9.0)
    ax.set_ylim(0, 2400)
    ax.grid(axis='y', linestyle='--', alpha=0.4, color=BORDER)

    for bar in rects1:
        yval = bar.get_height()
        ax.text(bar.get_x() + bar.get_width()/2, yval + 35, f'{yval}ms', ha='center', va='bottom',
                fontsize=8.0, fontweight='bold', color=NAVY)

    for bar in rects2:
        yval = bar.get_height()
        ax.text(bar.get_x() + bar.get_width()/2, yval + 35, f'{yval}ms', ha='center', va='bottom',
                fontsize=7.8, color=TEAL)

    ax.legend(frameon=True, facecolor=WHITE, edgecolor=BORDER, fontsize=9.0)
    plt.tight_layout()

    out_path = OUT_DIR / "fig4_8_gossip_convergence.png"
    plt.savefig(out_path)
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 4.9: Ingress Routing Throughput and Latency Comparison
# ==============================================================================
def make_fig4_9():
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 5.5), dpi=200)
    fig.patch.set_facecolor(WHITE)

    proxies = ['Nginx\n(L7)', 'Traefik\n(L7)', 'BRIDGE M1\n(Direct L7)', 'BRIDGE M2\n(Handoff L4)']
    rps = [38400, 26100, 34200, 47800]
    p99_latency = [4.2, 6.8, 4.6, 1.6]
    colors = [SLATE, "#5A6E7F", TEAL, NAVY]

    # Subplot 1: Throughput (RPS)
    ax1.set_facecolor(WHITE)
    bars1 = ax1.bar(proxies, rps, color=colors, width=0.55, edgecolor=NAVY)
    ax1.set_ylabel('Requests Per Second (RPS)', fontsize=10.0, fontweight='bold', color=NAVY)
    ax1.set_title('Throughput Capacity (Higher is Better)', fontsize=10.5, fontweight='bold', color=NAVY)
    ax1.set_ylim(0, 56000)
    ax1.grid(axis='y', linestyle='--', alpha=0.4, color=BORDER)

    for bar in bars1:
        y = bar.get_height()
        ax1.text(bar.get_x() + bar.get_width()/2, y + 800, f'{y:,}\nRPS', ha='center', va='bottom',
                 fontsize=8.2, fontweight='bold', color=NAVY)

    # Subplot 2: 99th %tile Latency (ms)
    ax2.set_facecolor(WHITE)
    bars2 = ax2.bar(proxies, p99_latency, color=colors, width=0.55, edgecolor=NAVY)
    ax2.set_ylabel('99th Percentile Latency (Milliseconds)', fontsize=10.0, fontweight='bold', color=NAVY)
    ax2.set_title('P99 Latency (Lower is Better)', fontsize=10.5, fontweight='bold', color=NAVY)
    ax2.set_ylim(0, 8.5)
    ax2.grid(axis='y', linestyle='--', alpha=0.4, color=BORDER)

    for bar in bars2:
        y = bar.get_height()
        ax2.text(bar.get_x() + bar.get_width()/2, y + 0.15, f'{y:.1f} ms', ha='center', va='bottom',
                 fontsize=8.2, fontweight='bold', color=NAVY)

    fig.suptitle('Figure 4.9: Ingress Routing Throughput and P99 Latency Benchmark Comparison',
                 fontsize=12.0, fontweight='bold', color=NAVY, y=0.98)
    plt.tight_layout()

    out_path = OUT_DIR / "fig4_9_throughput_latency.png"
    plt.savefig(out_path)
    plt.close(fig)
    print("Saved:", out_path)


# ==============================================================================
# Figure 4.10: Failover Recovery Time across HA Tiers
# ==============================================================================
def make_fig4_10():
    # Two-panel balanced layout: Left = Automated Tiers (Linear Seconds), Right = Impact Comparison Table
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 5.5), dpi=200, gridspec_kw={'width_ratios': [1.3, 1.0]})
    fig.patch.set_facecolor(WHITE)
    ax1.set_facecolor(WHITE)
    ax2.set_facecolor(WHITE)

    # Panel 1: Automated High-Availability Tiers (Linear 0-35s)
    tiers = [
        'Tier 3b: Floating IP (Hetzner API)',
        'Tier 3a: Warm Tunnel (QUIC Switch)',
        'Tier 3a: Cold Tunnel (Process Boot)',
        'Tier 2: Dynamic DNS (Min TTL Floor)'
    ]
    swim = np.array([1.5, 1.5, 1.5, 1.5])
    bully = np.array([1.2, 1.1, 1.1, 1.2])
    handoff = np.array([1.4, 1.8, 8.6, 27.3])
    totals = swim + bully + handoff

    y = np.arange(len(tiers))
    h = 0.52

    p1 = ax1.barh(y, swim, h, label='SWIM Failure Detection (1.5s)', color=NAVY)
    p2 = ax1.barh(y, bully, h, left=swim, label='Bully Leader Election (1.1–1.2s)', color=TEAL)
    p3 = ax1.barh(y, handoff, h, left=swim + bully, label='Ingress Route Handoff', color=SLATE)

    ax1.set_xlabel('Recovery Duration (Seconds — Linear Scale)', fontsize=9.5, fontweight='bold', color=NAVY)
    ax1.set_title('Automated Ingress Failover Latency Breakdown', fontsize=10.5, fontweight='bold', color=NAVY, pad=10)
    ax1.set_yticks(y)
    ax1.set_yticklabels(tiers, fontsize=8.5)
    ax1.set_xlim(0, 36)
    ax1.grid(axis='x', linestyle='--', alpha=0.4, color=BORDER)

    for idx, tot in enumerate(totals):
        ax1.text(tot + 0.7, idx, f'{tot:.1f} s', va='center', ha='left', fontsize=8.2, fontweight='bold', color=NAVY)

    ax1.legend(loc='lower right', frameon=True, facecolor=WHITE, edgecolor=BORDER, fontsize=8.0)

    # Panel 2: Comparative Operational Impact (including Manual Baseline)
    ax2.set_axis_off()
    ax2.set_title('Architectural Tier Resilience Summary', fontsize=10.5, fontweight='bold', color=NAVY, pad=10)

    # Clean academic summary table on ax2
    summary_data = [
        ["Failover Tier", "Downtime", "Resilience Level", "Human Action"],
        ["Tier 3b (Floating IP)", "4.1 s", "Sub-5s Instant", "Zero (Automated)"],
        ["Tier 3a (Warm Tunnel)", "4.4 s", "Sub-5s Instant", "Zero (Automated)"],
        ["Tier 3a (Cold Tunnel)", "11.2 s", "Near-Instant", "Zero (Automated)"],
        ["Tier 2 (Dynamic DNS)", "30.0 s", "Floor Capped", "Zero (Automated)"],
        ["Tier 1 (Manual Ops)", "> 300 s", "Unprotected", "Manual Redirect"]
    ]

    table = ax2.table(cellText=summary_data, loc='center', cellLoc='center',
                      colWidths=[0.38, 0.20, 0.24, 0.24])
    table.auto_set_font_size(False)
    table.set_fontsize(8.0)
    table.scale(1.0, 1.8)

    for (r, c), cell in table.get_celld().items():
        cell.set_edgecolor(BORDER)
        cell.set_linewidth(0.8)
        if r == 0:
            cell.set_facecolor(NAVY)
            cell.set_text_props(color=WHITE, fontweight='bold')
        elif r == 5:
            cell.set_facecolor("#FCEDED") # subtle warning tint for manual baseline
            cell.set_text_props(color=NAVY)
        else:
            cell.set_facecolor(LIGHT_BG if r % 2 == 1 else WHITE)
            cell.set_text_props(color=NAVY)

    fig.suptitle('Figure 4.10: High-Availability Failover Recovery Duration by Entry-Point Tier',
                 fontsize=12.0, fontweight='bold', color=NAVY, y=0.98)
    plt.tight_layout()

    out_path = OUT_DIR / "fig4_10_failover_recovery.png"
    plt.savefig(out_path)
    plt.close(fig)
    print("Saved:", out_path)


if __name__ == "__main__":
    make_fig2_1()
    make_fig2_2()
    make_fig3_1()
    make_fig3_2()
    make_fig3_3()
    make_fig3_4()
    make_fig4_8()
    make_fig4_9()
    make_fig4_10()
    print("All 9 report figures generated successfully!")
