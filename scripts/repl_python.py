#!/usr/bin/env python3
"""Persistent Python REPL server for HyperAgent.
Maintains global state across calls. Each invocation is a separate process
that loads previous history, executes the new code, appends to history, and
returns structured JSON output."""

import sys
import json
import io
import os

HISTORY_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "repl_python")
os.makedirs(HISTORY_DIR, exist_ok=True)
HISTORY_FILE = os.path.join(HISTORY_DIR, "history.py")

# Global state dictionary — persists across calls via history replay
globals_dict = {
    "__builtins__": __builtins__,
    "json": json,
    "os": os,
}

# Load and replay history to restore state
if os.path.exists(HISTORY_FILE):
    with open(HISTORY_FILE) as f:
        history = f.read()
    try:
        exec(history, globals_dict)
    except Exception:
        pass  # Don't error on stale history

# Read code from stdin
code = sys.stdin.read()

# Capture stdout/stderr
stdout_capture = io.StringIO()
stderr_capture = io.StringIO()
old_stdout, old_stderr = sys.stdout, sys.stderr
sys.stdout, sys.stderr = stdout_capture, stderr_capture

try:
    exec(code, globals_dict)
except Exception as e:
    print(f"Error: {type(e).__name__}: {e}", file=sys.stderr)

sys.stdout, sys.stderr = old_stdout, old_stderr

# Append to history for next call
with open(HISTORY_FILE, "a") as f:
    f.write("\n" + code)

# Output structured JSON
result = {
    "stdout": stdout_capture.getvalue(),
    "stderr": stderr_capture.getvalue(),
}
print(json.dumps(result), end="")
