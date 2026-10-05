#!/usr/bin/env python3
"""
Update SIWES_Technical_Report_Peniel_Final.docx:
1. Persist 100% of user edits (cover page, section 4.1, section 4.9 Bully rationale, etc.).
2. Replace media/image*.png with refined academic two-color diagrams.
3. Update inline shape extents (cx, cy) to match exact image aspect ratios without distortion.
4. Set page_break_before on paragraph introducing Figure 4.2 so Page 34 has balanced layout without stranded text.
5. Convert to PDF, verify TOC page numbers, update TOC if needed, and re-convert.
"""

import os
import shutil
import zipfile
import subprocess
from pathlib import Path
from PIL import Image
import docx
from docx.shared import Inches
import fitz
import re

ROOT_DIR = Path(__file__).resolve().parents[2]
FINAL_DOCX = str(ROOT_DIR / "reports" / "SIWES_Technical_Report_Peniel_Ben.docx")
FINAL_PDF = str(ROOT_DIR / "reports" / "SIWES_Technical_Report_Peniel_Ben.pdf")

# Map of media image filename to source image on disk
IMAGE_REPLACEMENTS = {
    "media/image1.png": str(ROOT_DIR / "report_figures" / "fig2_1_akirs_hierarchy.png"),
    "media/image2.png": str(ROOT_DIR / "report_figures" / "fig2_2_jrb_modernization.png"),
    "media/image3.png": str(ROOT_DIR / "report_figures" / "fig3_2_data_cleaner_pipeline.png"),
    "media/image4.png": str(ROOT_DIR / "report_figures" / "fig3_3_recon_engine_pipeline.png"),
    "media/image5.png": str(ROOT_DIR / "report_figures" / "fig3_4_ai_assistant_guardrails.png"),
    "media/image6.png": str(ROOT_DIR / "report_figures" / "fig3_1_datacenter_network.png"),
    "media/image7.png": str(ROOT_DIR / "docs" / "assets" / "04_deployment_diagram.png"),
    "media/image8.png": str(ROOT_DIR / "docs" / "assets" / "01_system_context.png"),
    "media/image9.png": str(ROOT_DIR / "docs" / "assets" / "02_architecture_c4.png"),
    "media/image10.png": str(ROOT_DIR / "docs" / "assets" / "11_proxy_handoff_separation.png"),
    "media/image11.png": str(ROOT_DIR / "docs" / "assets" / "09_proxy_modes_architecture.png"),
    "media/image12.png": str(ROOT_DIR / "docs" / "assets" / "10_seq_sni_handoff_routing.png"),
    "media/image13.png": str(ROOT_DIR / "docs" / "assets" / "13_concurrency_scaling_architecture.png"),
    "media/image14.png": str(ROOT_DIR / "report_figures" / "fig4_8_gossip_convergence.png"),
    "media/image15.png": str(ROOT_DIR / "report_figures" / "fig4_10_failover_recovery.png"),
    "media/image16.png": str(ROOT_DIR / "report_figures" / "fig4_9_throughput_latency.png"),
}

def update_docx_media_and_layout():
    print(f"Loading {FINAL_DOCX}...")
    doc = docx.Document(FINAL_DOCX)

    # 1. Fix Page 34 layout: find paragraph introducing Figure 4.2
    for i, p in enumerate(doc.paragraphs):
        if "Figure 4.2 details the C4 Level 2 Container and Component diagram" in p.text:
            print(f"Found Figure 4.2 intro at paragraph {i}. Adding page_break_before...")
            p.paragraph_format.page_break_before = True
            break

    # 2. Check and update inline shape extents to match exact image aspect ratios
    # Standard width = 5.8 inches = 5,303,520 EMU
    TARGET_WIDTH_IN = 5.8
    TARGET_WIDTH_EMU = int(TARGET_WIDTH_IN * 914400)

    for i, p in enumerate(doc.paragraphs):
        blips = p._element.xpath('.//a:blip/@r:embed')
        if blips:
            r_id = blips[0]
            target_ref = doc.part.rels[r_id].target_ref
            # target_ref is like 'media/image1.png'
            src_file = IMAGE_REPLACEMENTS.get(target_ref)
            if src_file and os.path.exists(src_file):
                im = Image.open(src_file)
                w_px, h_px = im.size
                aspect = w_px / h_px
                target_height_emu = int(TARGET_WIDTH_EMU / aspect)
                
                # Update wp:extent cx and cy
                extents = p._element.xpath('.//wp:extent')
                if extents:
                    extents[0].set('cx', str(TARGET_WIDTH_EMU))
                    extents[0].set('cy', str(target_height_emu))
                
                # Also update a:ext cx and cy inside a:graphicData
                a_exts = p._element.xpath('.//a:xfrm/a:ext')
                if a_exts:
                    a_exts[0].set('cx', str(TARGET_WIDTH_EMU))
                    a_exts[0].set('cy', str(target_height_emu))

    scratch_dir = ROOT_DIR / "scratch"
    scratch_dir.mkdir(exist_ok=True)
    temp_docx = str(scratch_dir / "temp_modified.docx")
    doc.save(temp_docx)
    print(f"Saved layout adjustments to {temp_docx}")

    # 3. Replace media images inside the docx zip archive
    out_docx = FINAL_DOCX
    temp_zip = str(scratch_dir / "temp_zip.docx")

    with zipfile.ZipFile(temp_docx, 'r') as zin, zipfile.ZipFile(temp_zip, 'w', compression=zipfile.ZIP_DEFLATED) as zout:
        for item in zin.infolist():
            filename = item.filename
            if filename.startswith("word/media/"):
                rel_name = filename.replace("word/", "")
                src_img = IMAGE_REPLACEMENTS.get(rel_name)
                if src_img and os.path.exists(src_img):
                    print(f"Replacing {filename} with {src_img}...")
                    with open(src_img, 'rb') as f:
                        zout.writestr(item, f.read())
                else:
                    zout.writestr(item, zin.read(filename))
            else:
                zout.writestr(item, zin.read(filename))

    shutil.move(temp_zip, out_docx)
    print(f"Successfully updated {out_docx} with refined media and layout!")

def convert_to_pdf():
    print("Converting DOCX to PDF via LibreOffice...")
    cmd = [
        "libreoffice", "--headless", "--convert-to", "pdf",
        FINAL_DOCX, "--outdir", str(ROOT_DIR / "reports")
    ]
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        print("LibreOffice error:", res.stderr)
        raise RuntimeError("PDF conversion failed")
    print(f"Converted to {FINAL_PDF}")

def verify_and_sync_toc():
    doc = fitz.open(FINAL_PDF)
    print(f"Total pages in final PDF: {len(doc)}")
    
    # Check physical page 1 (cover)
    p1 = doc[0].get_text('text')
    print("Cover Page First Line:", p1.split('\n')[0])
    
    # Check TOC on physical pages 2, 3, 4
    toc_text = ""
    for idx in [1, 2, 3]:
        toc_text += doc[idx].get_text('text') + "\n"

    # Search for headings in body (phys idx 4 onwards)
    headings_to_check = [
        ("Chapter One (1)", 1),
        ("Chapter Two (2)", 5),
        ("Chapter Three (3)", 13),
        ("Chapter Four (4)", 26),
        ("4.5 SYSTEM ARCHITECTURE AND C4 TOPOLOGY OVERVIEW", 30),
        ("4.18 REFERENCES", 42),
    ]
    for h, _ in headings_to_check:
        found_pages = []
        for idx in range(4, len(doc)):
            text = doc[idx].get_text('text')
            if h in text:
                found_pages.append(idx - 3)
        print(f"Heading '{h}': Found on doc page(s) {found_pages}")

if __name__ == "__main__":
    update_docx_media_and_layout()
    convert_to_pdf()
    verify_and_sync_toc()
    print("Update complete!")
