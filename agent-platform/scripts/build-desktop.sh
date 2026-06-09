#!/bin/bash
# build-desktop.sh — Build desktop app for Mac + Windows
# macOS .dmg:     bash scripts/build-desktop.sh mac
# Windows .msi:   bash scripts/build-desktop.sh win

TARGET=${1:-mac}
echo "🔨 Building desktop for $TARGET..."

cd agent-platform/frontend

# Build frontend
npm run build

# Build Tauri
if [ "$TARGET" = "win" ]; then
    npx @tauri-apps/cli build --target x86_64-pc-windows-msvc
    echo "✅ Installer: src-tauri/target/release/bundle/msi/*.msi"
else
    npx @tauri-apps/cli build --target aarch64-apple-darwin --target x86_64-apple-darwin
    echo "✅ Installer: src-tauri/target/release/bundle/dmg/*.dmg"
fi
