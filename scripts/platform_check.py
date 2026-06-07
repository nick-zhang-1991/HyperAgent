#!/usr/bin/env python3
"""Cross-platform toolchain checker and installer for HyperAgent.
Detects what's available on each platform and gives install instructions.
Outputs JSON for consumption by the agent."""

import sys
import json
import os
import shutil
import subprocess
import platform


def check_python():
    """Check Python3 availability and key packages"""
    result = {"available": False, "version": "", "packages": {}}
    py = shutil.which("python3") or shutil.which("python")
    if py:
        try:
            v = subprocess.run([py, "--version"], capture_output=True, text=True, timeout=5)
            result["available"] = True
            result["version"] = v.stdout.strip() or v.stderr.strip()
            result["path"] = py
        except Exception:
            pass

    # Check key packages
    for pkg in ["pandas", "openpyxl", "python-docx", "pymupdf", "websocket-client"]:
        try:
            r = subprocess.run([py or "python3", "-c", f"import {pkg.split('-')[-1]}; print('ok')"],
                              capture_output=True, text=True, timeout=5)
            result["packages"][pkg] = r.stdout.strip() == "ok"
        except Exception:
            result["packages"][pkg] = False
    return result


def check_chrome():
    """Check Chrome/Chromium availability"""
    if sys.platform == "darwin":
        paths = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            os.path.expanduser("~/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
    elif sys.platform == "linux":
        paths = shutil.which("google-chrome") or shutil.which("chromium-browser") or shutil.which("chromium") or ""
        paths = [paths] if isinstance(paths, str) else paths
    elif sys.platform == "win32":
        paths = [
            os.path.expandvars(r"%ProgramFiles%\Google\Chrome\Application\chrome.exe"),
            os.path.expandvars(r"%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe"),
            os.path.expandvars(r"%LocalAppData%\Google\Chrome\Application\chrome.exe"),
        ]
    else:
        paths = []

    for p in paths:
        if p and os.path.exists(p):
            return {"available": True, "path": p}
    return {"available": False, "path": None}


def check_pdftotext():
    """Check pdftotext availability"""
    p = shutil.which("pdftotext")
    if p:
        return {"available": True, "path": p}
    return {"available": False, "path": None}


def install_instructions():
    """Generate platform-specific install instructions"""
    os_name = platform.system()
    instructions = []

    if os_name == "Darwin":
        instructions.append("📦 macOS setup:")
        instructions.append("  brew install poppler              # PDF support (pdftotext)")
        instructions.append("  pip3 install pandas openpyxl python-docx pymupdf websocket-client")
        instructions.append("  # Chrome is pre-installed on most Macs")

    elif os_name == "Linux":
        instructions.append("📦 Linux setup:")
        instructions.append("  sudo apt-get install -y poppler-utils   # pdftotext for PDF")
        instructions.append("  sudo apt-get install -y chromium-browser || sudo apt-get install -y google-chrome-stable")
        instructions.append("  pip3 install pandas openpyxl python-docx pymupdf websocket-client")
        instructions.append("  # Also: sudo apt-get install xdotool scrot  (for desktop automation)")

    elif os_name == "Windows":
        instructions.append("📦 Windows setup:")
        instructions.append("  # Install Python packages:")
        instructions.append("  pip install pandas openpyxl python-docx pymupdf websocket-client")
        instructions.append("  # Install poppler for PDF:")
        instructions.append("  # Download from: https://github.com/oschwartz10612/poppler-windows/releases")
        instructions.append("  # Add bin/ to PATH")
        instructions.append("  # Chrome is usually pre-installed on Windows")
        instructions.append("  # For desktop automation: PowerShell is built-in")

    else:
        instructions.append(f"Unknown OS: {os_name}")

    return "\n".join(instructions)


def main():
    os_ = platform.system()
    arch = platform.machine()

    result = {
        "os": os_,
        "arch": arch,
        "python": check_python(),
        "chrome": check_chrome(),
        "pdftotext": check_pdftotext(),
        "install_guide": install_instructions(),
        "tools_available": [],
        "tools_missing": [],
    }

    # Compile available/missing lists
    tool_map = {
        ("python3", "python"): "Python REPL",
        ("chrome", "chrome"): "Browser automation",
        ("pdftotext", "pdftotext"): "PDF parsing",
    }

    for (key, _), name in tool_map.items():
        if result.get(key, {}).get("available"):
            result["tools_available"].append(name)
        else:
            result["tools_missing"].append(name)

    # Check Python packages
    for pkg, ok in result.get("python", {}).get("packages", {}).items():
        if ok:
            result["tools_available"].append(f"Python {pkg}")
        else:
            result["tools_missing"].append(f"Python {pkg}")

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
