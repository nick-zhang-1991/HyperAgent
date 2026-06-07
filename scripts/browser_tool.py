#!/usr/bin/env python3
"""Browser tool for HyperAgent. Controls Chrome/Chromium headless.
Supports: open URL, get page text, screenshot, click, JavaScript eval.
Communication via Chrome DevTools Protocol (CDP) over WebSocket.

Usage:
  browser.py open <url> [--timeout N]        # Navigate, return page text
  browser.py screenshot <path>                # Take screenshot
  browser.py eval <js_code>                   # Run JS in page
  browser.py click <selector>                 # Click element
  browser.py source                           # Get page HTML/text
  browser.py close                            # Close browser

State is maintained via a persistent Chrome process.
"""

import sys
import json
import os
import subprocess
import tempfile
import time
import shutil
import socket
import urllib.request
import urllib.error


def find_chrome():
    """Find Chrome/Chromium executable"""
    candidates = []
    if sys.platform == "darwin":
        candidates = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            os.path.expanduser("~/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        ]
    elif sys.platform == "linux":
        candidates = shutil.which("google-chrome") or shutil.which("chromium-browser") or shutil.which("chromium") or []
        if candidates:
            candidates = [candidates] if isinstance(candidates, str) else candidates
        else:
            candidates = ["/usr/bin/google-chrome", "/usr/bin/chromium-browser", "/usr/bin/chromium"]
    elif sys.platform == "win32":
        candidates = [
            os.path.expandvars(r"%ProgramFiles%\Google\Chrome\Application\chrome.exe"),
            os.path.expandvars(r"%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe"),
            os.path.expandvars(r"%LocalAppData%\Google\Chrome\Application\chrome.exe"),
        ]

    for c in candidates:
        if c and os.path.exists(c):
            return c
    return None


def get_chrome_process(data_dir):
    """Start or find Chrome with remote debugging"""
    port_file = os.path.join(data_dir, "debug_port.txt")
    pid_file = os.path.join(data_dir, "chrome_pid.txt")

    # Check if already running
    if os.path.exists(pid_file):
        with open(pid_file) as f:
            old_pid = f.read().strip()
        if old_pid:
            try:
                os.kill(int(old_pid), 0)  # Check if alive
                with open(port_file) as pf:
                    port = int(pf.read().strip())
                return port, int(old_pid)
            except (OSError, ValueError):
                pass  # Process dead, restart

    chrome_path = find_chrome()
    if not chrome_path:
        return None, None

    # Find a free port
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]

    # Launch Chrome
    user_data_dir = os.path.join(data_dir, "chrome_profile")
    os.makedirs(user_data_dir, exist_ok=True)

    cmd = [
        chrome_path,
        f"--remote-debugging-port={port}",
        f"--user-data-dir={user_data_dir}",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-sync",
        "--disable-translate",
        "--disable-background-networking",
        "--no-sandbox" if sys.platform == "linux" else "--no-sandbox",
    ]

    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        with open(pid_file, "w") as f:
            f.write(str(proc.pid))
        with open(port_file, "w") as f:
            f.write(str(port))

        # Wait for debugger to be ready
        for _ in range(30):
            try:
                resp = urllib.request.urlopen(f"http://127.0.0.1:{port}/json/version", timeout=2)
                if resp.status == 200:
                    break
            except (urllib.error.URLError, socket.timeout):
                pass
            time.sleep(0.5)

        return port, proc.pid
    except Exception as e:
        return None, str(e)


def get_ws_url(port):
    """Get WebSocket debugger URL"""
    try:
        resp = urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=5)
        data = json.loads(resp.read())
        if data:
            return data[0].get("webSocketDebuggerUrl", "")
    except Exception:
        pass
    return ""


def open_url(port, url, timeout=15):
    """Navigate to URL via CDP HTTP endpoint and return page text"""
    ws_url = get_ws_url(port)
    if not ws_url:
        return {"error": "Could not get WebSocket debugger URL"}

    try:
        import websocket
        ws = websocket.create_connection(ws_url, timeout=timeout)

        # Navigate
        navigate_cmd = json.dumps({
            "id": 1,
            "method": "Page.navigate",
            "params": {"url": url}
        })
        ws.send(navigate_cmd)
        ws.recv()  # Navigation result

        # Wait for page load
        time.sleep(2)

        # Get page text via document.body.innerText
        eval_cmd = json.dumps({
            "id": 2,
            "method": "Runtime.evaluate",
            "params": {
                "expression": "document.body.innerText",
                "returnByValue": True
            }
        })
        ws.send(eval_cmd)
        result = ws.recv()
        ws.close()

        data = json.loads(result)
        text = data.get("result", {}).get("result", {}).get("value", "")
        return {"content": text[:50000], "truncated": len(text) > 50000}
    except ImportError:
        # Fallback: use --dump-dom
        chrome = find_chrome()
        if not chrome:
            return {"error": "Chrome not found"}
        data_dir = os.path.dirname(port_file_path())
        user_data_dir = os.path.join(data_dir, "chrome_profile")
        try:
            result = subprocess.run(
                [chrome, "--headless", "--dump-dom", "--no-sandbox",
                 f"--user-data-dir={user_data_dir}", url],
                capture_output=True, text=True, timeout=timeout
            )
            text = result.stdout
            if not text:
                text = result.stderr
            return {"content": text[:50000], "truncated": len(text) > 50000}
        except Exception as e:
            return {"error": str(e)}
    except Exception as e:
        return {"error": str(e)}


def take_screenshot(port, path):
    """Take screenshot via CDP"""
    ws_url = get_ws_url(port)
    if not ws_url:
        return {"error": "No WebSocket URL"}

    try:
        import websocket
        ws = websocket.create_connection(ws_url, timeout=15)

        cmd = json.dumps({
            "id": 1,
            "method": "Page.captureScreenshot",
            "params": {"format": "png"}
        })
        ws.send(cmd)
        result = ws.recv()
        ws.close()

        data = json.loads(result)
        b64_data = data.get("result", {}).get("data", "")
        if b64_data:
            import base64
            with open(path, "wb") as f:
                f.write(base64.b64decode(b64_data))
            return {"path": path, "size": len(b64_data)}
        return {"error": "No screenshot data"}
    except ImportError:
        # Fallback: --screenshot
        chrome = find_chrome()
        if not chrome:
            return {"error": "Chrome not found"}
        try:
            subprocess.run(
                [chrome, "--headless", "--screenshot=" + path,
                 "--window-size=1920,1080", "--no-sandbox", "about:blank"],
                capture_output=True, timeout=15
            )
            if os.path.exists(path):
                return {"path": path, "size": os.path.getsize(path)}
            return {"error": "Screenshot failed"}
        except Exception as e:
            return {"error": str(e)}
    except Exception as e:
        return {"error": str(e)}


def port_file_path():
    """Get the data directory path"""
    base = os.environ.get("HYPER_HOME", os.path.join(os.path.expanduser("~"), ".hyper"))
    return os.path.join(base, "tmp")


def main():
    if len(sys.argv) < 2:
        print(json.dumps({"error": "Usage: browser.py <command> [args...]"}))
        sys.exit(1)

    command = sys.argv[1]
    data_dir = port_file_path()
    os.makedirs(data_dir, exist_ok=True)

    if command == "close":
        pid_file = os.path.join(data_dir, "chrome_pid.txt")
        if os.path.exists(pid_file):
            with open(pid_file) as f:
                pid = f.read().strip()
            try:
                os.kill(int(pid), 15)
            except (OSError, ValueError):
                pass
            os.remove(pid_file)
        port_file = os.path.join(data_dir, "debug_port.txt")
        if os.path.exists(port_file):
            os.remove(port_file)
        print(json.dumps({"status": "closed"}))
        return

    # Ensure Chrome is running
    port, pid = get_chrome_process(data_dir)
    if port is None:
        print(json.dumps({"error": f"Chrome/Chromium not found. {pid or ''}"}))
        sys.exit(1)

    if command == "open":
        url = sys.argv[2] if len(sys.argv) > 2 else "about:blank"
        timeout = int(sys.argv[4]) if len(sys.argv) > 4 and sys.argv[3] == "--timeout" else 15
        if not url.startswith(("http://", "https://", "file://", "about:")):
            url = "https://" + url
        result = open_url(port, url, timeout)
        print(json.dumps(result))

    elif command == "screenshot":
        path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(data_dir, "screenshot.png")
        result = take_screenshot(port, path)
        print(json.dumps(result))

    elif command == "eval":
        js = sys.argv[2] if len(sys.argv) > 2 else "''"
        ws_url = get_ws_url(port)
        if not ws_url:
            print(json.dumps({"error": "No WebSocket URL"}))
            sys.exit(1)
        try:
            import websocket
            ws = websocket.create_connection(ws_url, timeout=15)
            cmd = json.dumps({
                "id": 1,
                "method": "Runtime.evaluate",
                "params": {"expression": js, "returnByValue": True}
            })
            ws.send(cmd)
            result = ws.recv()
            ws.close()
            data = json.loads(result)
            value = data.get("result", {}).get("result", {})
            print(json.dumps({"result": value.get("value", ""), "type": value.get("type", "")}))
        except ImportError:
            print(json.dumps({"error": "websocket-client not installed. pip install websocket-client"}))
        except Exception as e:
            print(json.dumps({"error": str(e)}))

    elif command == "source":
        ws_url = get_ws_url(port)
        if not ws_url:
            print(json.dumps({"error": "No WebSocket URL"}))
            sys.exit(1)
        try:
            import websocket
            ws = websocket.create_connection(ws_url, timeout=15)
            cmd = json.dumps({
                "id": 1,
                "method": "Runtime.evaluate",
                "params": {
                    "expression": "document.documentElement.outerHTML",
                    "returnByValue": True
                }
            })
            ws.send(cmd)
            result = ws.recv()
            ws.close()
            data = json.loads(result)
            html = data.get("result", {}).get("result", {}).get("value", "")
            print(json.dumps({"html": html[:50000], "truncated": len(html) > 50000}))
        except ImportError:
            print(json.dumps({"error": "websocket-client not installed. pip install websocket-client"}))
        except Exception as e:
            print(json.dumps({"error": str(e)}))

    else:
        print(json.dumps({"error": f"Unknown command: {command}"}))


if __name__ == "__main__":
    main()
