#!/usr/bin/env python3
"""Document parser for HyperAgent. Parses PDF, DOCX, and XLSX files to text.
Usage: python3 parse_document.py <filepath>
Returns JSON: {"content": "...", "error": "...", "pages": N, "format": "pdf|docx|xlsx|txt"}"""

import sys
import json
import os
import subprocess
import tempfile


def parse_pdf_pdftotext(path):
    """Try pdftotext (fast, no Python dep)"""
    try:
        result = subprocess.run(
            ["pdftotext", "-layout", path, "-"],
            capture_output=True, text=True, timeout=30
        )
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout, "pdftotext"
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    return None, None


def parse_pdf_pymupdf(path):
    """Try PyMuPDF (fitz)"""
    try:
        import fitz
        doc = fitz.open(path)
        text = []
        for page in doc:
            text.append(page.get_text())
        doc.close()
        return "\n\n--- Page Break ---\n\n".join(text), "pymupdf"
    except ImportError:
        pass
    return None, None


def parse_pdf_pdfminer(path):
    """Try pdfminer.six"""
    try:
        from pdfminer.high_level import extract_text
        text = extract_text(path)
        return text, "pdfminer"
    except ImportError:
        pass
    return None, None


def parse_pdf_six(path):
    """Try pdfminer.six (alternative import)"""
    try:
        from pdfminer.high_level import extract_text_to_fp
        from pdfminer.layout import LAParams
        from io import StringIO
        output = StringIO()
        with open(path, "rb") as f:
            extract_text_to_fp(f, output, laparams=LAParams(), output_type="text")
        return output.getvalue(), "pdfminer"
    except ImportError:
        pass
    return None, None


def parse_docx(path):
    """Parse .docx files using python-docx"""
    try:
        import docx
        doc = docx.Document(path)
        lines = []
        for para in doc.paragraphs:
            lines.append(para.text)
        # Also extract tables
        for table in doc.tables:
            lines.append("\n[TABLE]")
            for row in table.rows:
                cells = [cell.text.strip() for cell in row.cells]
                lines.append(" | ".join(cells))
            lines.append("[/TABLE]")
        return "\n".join(lines), "python-docx"
    except ImportError:
        pass
    return None, None


def parse_xlsx_pandas(path):
    """Parse .xlsx files using pandas/openpyxl"""
    try:
        import pandas as pd
        xls = pd.ExcelFile(path)
        sheets = []
        for sheet_name in xls.sheet_names:
            df = pd.read_excel(xls, sheet_name=sheet_name)
            sheets.append(f"=== Sheet: {sheet_name} ===\n{df.to_string(index=False)}")
        return "\n\n".join(sheets), "pandas"
    except ImportError:
        pass
    return None, None


def parse_xlsx_openpyxl(path):
    """Parse .xlsx using openpyxl directly"""
    try:
        import openpyxl
        wb = openpyxl.load_workbook(path, read_only=True, data_only=True)
        sheets = []
        for sheet_name in wb.sheetnames:
            ws = wb[sheet_name]
            rows = []
            for row in ws.iter_rows(values_only=True):
                cells = [str(c) if c is not None else "" for c in row]
                rows.append(" | ".join(cells))
            sheets.append(f"=== Sheet: {sheet_name} ===\n" + "\n".join(rows))
        wb.close()
        return "\n\n".join(sheets), "openpyxl"
    except ImportError:
        pass
    return None, None


def parse_txt(path):
    """Read plain text files"""
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            content = f.read()
        return content, "text"
    except Exception as e:
        return None, f"Error: {e}"


def main():
    if len(sys.argv) < 2:
        print(json.dumps({"error": "Usage: parse_document.py <filepath>"}))
        sys.exit(1)

    path = sys.argv[1]
    if not os.path.exists(path):
        print(json.dumps({"error": f"File not found: {path}"}))
        sys.exit(1)

    ext = os.path.splitext(path)[1].lower()
    file_size = os.path.getsize(path)

    # Handle by extension
    content = None
    engine = None

    if ext == ".pdf":
        # Try fastest first
        content, engine = parse_pdf_pdftotext(path)
        if content is None:
            content, engine = parse_pdf_pymupdf(path)
        if content is None:
            content, engine = parse_pdf_pdfminer(path)

    elif ext == ".docx":
        content, engine = parse_docx(path)

    elif ext in (".xlsx", ".xls"):
        content, engine = parse_xlsx_pandas(path)
        if content is None:
            content, engine = parse_xlsx_openpyxl(path)

    elif ext in (".txt", ".md", ".json", ".yaml", ".yml", ".toml", ".csv",
                 ".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".css", ".html",
                 ".xml", ".sh", ".bash", ".env", ".ini", ".cfg", ".conf",
                 ".log", ".sql", ".rb", ".go", ".java", ".kt", ".swift"):
        content, engine = parse_txt(path)

    else:
        # Try as text first
        content, engine = parse_txt(path)
        if content is None:
            print(json.dumps({"error": f"Unsupported format: {ext}"}))
            sys.exit(1)

    if content is None:
        print(json.dumps({"error": f"Could not parse {ext} file. Install a parser: "
                                    "PDF: apt-get install poppler-utils or pip install pymupdf; "
                                    "DOCX: pip install python-docx; "
                                    "XLSX: pip install pandas openpyxl"}))
        sys.exit(1)

    # Truncate very large content
    max_chars = 100_000
    truncated = len(content) > max_chars
    if truncated:
        content = content[:max_chars] + f"\n\n[...truncated: {len(content)} chars -> {max_chars}]"

    result = {
        "content": content,
        "engine": engine,
        "format": ext.lstrip("."),
        "pages": content.count("\n") + 1,
        "truncated": truncated,
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
