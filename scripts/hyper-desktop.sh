#!/usr/bin/env bash
# ============================================================
# HyperAgent Desktop — macOS application wrapper
# ============================================================
# Creates HyperAgent.app that opens the dashboard in a dedicated
# Safari web app window with Dock icon and menu bar.
#
# Usage:
#   bash scripts/hyper-desktop.sh create  [--port 8081]
#   bash scripts/hyper-desktop.sh launch  [--port 8081]
#   bash scripts/hyper-desktop.sh remove
# ============================================================
set -euo pipefail

APP_NAME="HyperAgent"
PORT="${2:-8081}"
APP_DIR="$HOME/Applications/${APP_NAME}.app"
ICON_URL="https://raw.githubusercontent.com/nick-zhang-1991/HyperAgent/master/docs/hyper-icon.png"

create() {
    echo "📦 Creating ${APP_NAME}.app..."

    mkdir -p "$APP_DIR/Contents/MacOS"
    mkdir -p "$APP_DIR/Contents/Resources"

    # Create the launcher script (runs hyper dashboard + opens Safari)
    cat > "$APP_DIR/Contents/MacOS/${APP_NAME}" << 'LAUNCHER'
#!/usr/bin/env bash
set -euo pipefail

PORT="${1:-8081}"
LOG="$HOME/Library/Logs/hyperagent-dashboard.log"

# Find hyper binary (check common locations)
HYPER=""
for candidate in "$HOME/.local/bin/hyper" \
                 "/usr/local/bin/hyper" \
                 "/opt/homebrew/bin/hyper" \
                 "$(which hyper 2>/dev/null || true)"; do
    if [ -x "$candidate" ]; then
        HYPER="$candidate"
        break
    fi
done

if [ -z "$HYPER" ]; then
    # Try cargo
    if command -v cargo &>/dev/null && [ -f "$HOME/HyperAgent/target/release/hyperagent" ]; then
        HYPER="$HOME/HyperAgent/target/release/hyperagent"
    else
        osascript -e 'display dialog "HyperAgent binary not found.\nInstall: cargo install --path .\nor copy hyper to ~/.local/bin/" buttons ["OK"] default button 1 with icon stop'
        exit 1
    fi
fi

echo "[$(date)] Starting HyperAgent Dashboard on port $PORT" >> "$LOG"
"$HYPER" dashboard --port "$PORT" >> "$LOG" 2>&1 &

# Wait for dashboard to start
sleep 2

# Open in default browser
open "http://127.0.0.1:$PORT"

# Keep process alive for Dock
wait $!
LAUNCHER
    chmod +x "$APP_DIR/Contents/MacOS/${APP_NAME}"

    # Create Info.plist
    cat > "$APP_DIR/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>com.hyperagent.desktop</string>
    <key>CFBundleName</key>
    <string>${APP_NAME} Dashboard</string>
    <key>CFBundleVersion</key>
    <string>1.0.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSUIElement</key>
    <false/>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

    echo "✅ Created $APP_DIR"
    echo ""
    echo "   Now you can:"
    echo "     • Open HyperAgent from your Applications folder (double-click)"
    echo "     • Pin it to the Dock"
    echo "     • Or run: open $APP_DIR"
    echo ""
    echo "   To set a custom port:"
    echo "     ./scripts/hyper-desktop.sh launch --port 9090"
}

launch() {
    if [ ! -d "$APP_DIR" ]; then
        echo "❌ ${APP_NAME}.app not found. Run 'create' first."
        exit 1
    fi
    echo "🚀 Launching ${APP_NAME} Dashboard on port $PORT..."
    open -n "$APP_DIR" --args "$PORT"
}

remove() {
    if [ -d "$APP_DIR" ]; then
        rm -rf "$APP_DIR"
        echo "🗑️  Removed $APP_DIR"
    else
        echo "   Not installed."
    fi
}

case "${1:-help}" in
    create) create ;;
    launch) launch ;;
    remove) remove ;;
    *)
        echo "Usage: $0 {create|launch|remove} [--port PORT]"
        echo ""
        echo "  create    Generate HyperAgent.app in ~/Applications/"
        echo "  launch    Launch HyperAgent Dashboard"
        echo "  remove    Delete the .app bundle"
        ;;
esac
