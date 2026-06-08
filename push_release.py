#!/usr/bin/env python3
import subprocess, json

GH_TOKEN = "ghp_TQ...Zfa"
USER = "nick-zhang-1991"
REPO = "HyperAgent"
BASE = f"https://api.github.com/repos/{USER}/{REPO}"

def gh(method, path, data=None):
    cmd = ["curl", "-s", "-w", "\n%{http_code}",
           "-H", f"Authorization: token {GH_TOKEN}",
           "-H", "Accept: application/vnd.github.v3+json"]
    if method == "POST":
        cmd += ["-X", "POST", "-H", "Content-Type: application/json", "-d", json.dumps(data)]
    elif method == "DELETE":
        cmd += ["-X", "DELETE"]
    r = subprocess.run(cmd + [f"{BASE}{path}"], capture_output=True, text=True, timeout=30)
    out = r.stdout.strip()
    if not out:
        return {"_error": "empty response"}
    parts = out.rsplit("\n", 1)
    body, code = parts[0] if len(parts) > 1 else out, parts[-1]
    try:
        result = json.loads(body) if body.strip() else {}
        result["_code"] = code
        return result
    except:
        return {"_raw": out[:200], "_code": code}

# Verify token works
me = gh("GET", "/../../user")
print(f"Auth check: {me.get('login', me.get('message', 'unknown'))}")

if me.get('login') != USER:
    print("Token not valid! Exiting.")
    exit(1)

# 1. Push fix commits via Git Data API
# First get the current main branch ref
main = gh("GET", "/git/refs/heads/main")
print(f"Main SHA: {main['object']['sha'][:12]}")

# Delete old tag if exists
gh("DELETE", "/git/refs/tags/v0.1.0")

# We need to create a commit for desktop.yml and pnpm-lock changes
# Read the fixed files
with open(".github/workflows/desktop.yml") as f:
    desktop_yml = f.read()
with open("gui/pnpm-lock.yaml") as f:
    pnpm_lock = f.read()

# Create blobs
blob1 = gh("POST", "/git/blobs", {"content": desktop_yml, "encoding": "utf-8"})
print(f"Blob1 SHA: {blob1.get('sha','FAIL')[:12]}")
if 'sha' not in blob1:
    print(f"Blob1 error: {json.dumps(blob1)[:200]}")

blob2 = gh("POST", "/git/blobs", {"content": pnpm_lock, "encoding": "utf-8"})
print(f"Blob2 SHA: {blob2.get('sha','FAIL')[:12]}")

# Create tree
base_tree = gh("GET", f"/git/commits/{main['object']['sha']}")
base_tree_sha = base_tree['tree']['sha']
print(f"Base tree SHA: {base_tree_sha[:12]}")

tree_items = [
    {"path": ".github/workflows/desktop.yml", "mode": "100644", "type": "blob", "sha": blob1['sha']},
    {"path": "gui/pnpm-lock.yaml", "mode": "100644", "type": "blob", "sha": blob2['sha']},
]

new_tree = gh("POST", "/git/trees", {"base_tree": base_tree_sha, "tree": tree_items})
print(f"New tree SHA: {new_tree.get('sha','FAIL')[:12]}")

# Create commit
commit_data = {
    "message": "fix: desktop CI and pnpm-lock for v0.1.0 release",
    "tree": new_tree['sha'],
    "parents": [main['object']['sha']],
}
new_commit = gh("POST", "/git/commits", commit_data)
print(f"New commit SHA: {new_commit.get('sha','FAIL')[:12]}")

# Update main branch ref
gh("PATCH", "/git/refs/heads/main", {"sha": new_commit['sha'], "force": False})
print(f"Main branch updated!")

# Create tag
tag_data = {
    "tag": "v0.1.0",
    "message": "HyperAgent v0.1.0 - First Release",
    "object": new_commit['sha'],
    "type": "commit",
    "tagger": {"name": "HyperAgent", "email": "bot@hyperagent.dev"}
}
new_tag = gh("POST", "/git/tags", tag_data)
print(f"Tag SHA: {new_tag.get('sha','FAIL')[:12]}")

# Create tag ref
gh("POST", "/git/refs", {"ref": "refs/tags/v0.1.0", "sha": new_tag['sha']})
print(f"Tag ref created!")

# Create release
release_data = {
    "tag_name": "v0.1.0",
    "name": "HyperAgent v0.1.0",
    "body": (
        "## HyperAgent v0.1.0 - First Release\n\n"
        "### CLI\n"
        "Cross-platform (macOS/Linux/Windows). Download from assets below.\n\n"
        "### Desktop App (Tauri)\n"
        "Native GUI for macOS (.dmg), Linux (.AppImage), Windows (.msi).\n\n"
        "### Installation\n"
        "See [README](https://github.com/nick-zhang-1991/HyperAgent#readme) for details.\n"
    ),
    "draft": False,
    "prerelease": False,
}
release = gh("POST", "/releases", release_data)
print(f"Release: {release.get('html_url', release.get('message','?'))}")

print("\nDone! Check:", release.get('html_url', 'https://github.com/nick-zhang-1991/HyperAgent/releases'))
