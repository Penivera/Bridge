from pathlib import Path
#!/usr/bin/env python3
"""
Sync TOC page numbers in SIWES_Technical_Report_Peniel_Final.docx
using exact page coordinates from SIWES_Technical_Report_Peniel_Final.pdf.
"""
import re
import subprocess
import docx
import fitz

ROOT_DIR = Path(__file__).resolve().parents[2]
DOCX_PATH = str(ROOT_DIR / "reports" / "SIWES_Technical_Report_Peniel_Ben.docx")
PDF_PATH = str(ROOT_DIR / "reports" / "SIWES_Technical_Report_Peniel_Ben.pdf")

def sync_toc():
    # 1. Open PDF and find Chapter 1 physical page
    pdf = fitz.open(PDF_PATH)
    ch1_phys_page = 5
    for p_idx, page in enumerate(pdf):
        text = page.get_text("text")
        lines = [l.strip() for l in text.split("\n") if l.strip()]
        if "Chapter One (1)" in text and not any("..." in l or "…" in l for l in lines):
            ch1_phys_page = p_idx + 1
            break
    print(f"Chapter 1 starts at physical page {ch1_phys_page}")

    # 2. Open DOCX and get all TOC paragraphs
    doc = docx.Document(DOCX_PATH)
    toc_paras = []
    for i in range(20, 130):
        if i < len(doc.paragraphs):
            p = doc.paragraphs[i]
            if len(p.runs) >= 3 and p.runs[1].text == '\t':
                title = p.runs[0].text.strip()
                old_page = p.runs[2].text.strip()
                toc_paras.append((i, p, title, old_page))

    print(f"Found {len(toc_paras)} TOC entries in DOCX.")

    # 3. For each TOC title, find where it appears in PDF body (phys idx >= ch1_phys_page - 1)
    updated_count = 0
    for idx, p, title, old_page in toc_paras:
        # Clean title for searching
        clean = re.sub(r'^[0-9.]+\s*', '', title)
        # Search token: take first 25 characters or first 4 words
        words = clean.split()
        search_token = " ".join(words[:4]) if len(words) >= 4 else clean
        search_token = search_token.replace("—", "").replace("-", " ").strip()
        # First word or number prefix if applicable
        num_match = re.match(r'^([0-9.]+)', title)
        num_prefix = num_match.group(1) if num_match else ""

        found_page = None
        for p_idx in range(ch1_phys_page - 1, len(pdf)):
            page_text = pdf[p_idx].get_text("text")
            lines = [l.strip() for l in page_text.split("\n") if "..." not in l and "…" not in l]
            page_clean = " ".join(lines).lower().replace("—", "").replace("-", " ")
            
            # Check if title or (num_prefix and search_token) matches
            token_match = search_token.lower() in page_clean
            if num_prefix:
                exact_heading = (num_prefix.lower() + " " + search_token.lower()).strip()
                if exact_heading in page_clean or (num_prefix in page_clean.split() and token_match):
                    doc_page = (p_idx + 1) - ch1_phys_page + 1
                    found_page = str(doc_page)
                    break
            elif token_match:
                doc_page = (p_idx + 1) - ch1_phys_page + 1
                found_page = str(doc_page)
                break

        if found_page:
            if found_page != old_page:
                print(f"Updating p[{idx:3d}] '{title[:45]}': {old_page} -> {found_page}")
                p.runs[2].text = found_page
                updated_count += 1
        else:
            print(f"WARNING: Heading not found for '{title}' (kept {old_page})")

    print(f"Total updated TOC page numbers: {updated_count}")
    doc.save(DOCX_PATH)
    print(f"Saved synchronized DOCX to {DOCX_PATH}")

    # Reconvert to PDF
    print("Re-converting DOCX to PDF...")
    cmd = ["libreoffice", "--headless", "--convert-to", "pdf", DOCX_PATH, "--outdir", str(ROOT_DIR / "reports")]
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        print("LibreOffice error:", res.stderr)
        raise RuntimeError("PDF conversion failed")
    print(f"Re-converted to {PDF_PATH}")

if __name__ == "__main__":
    sync_toc()
