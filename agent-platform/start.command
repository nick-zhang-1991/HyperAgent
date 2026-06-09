#!/bin/bash
# Agent Platform — double-click to launch

DIR="$(cd "$(dirname "$0")" && pwd)"
echo "🚀 Agent Platform starting..."

# Start backend
"$DIR/agent-platform/backend/target/debug/agent-platform" &
sleep 2

# Serve pre-built frontend (no compile needed)
cd "$DIR/agent-platform/frontend"
npx vite preview --host 0.0.0.0 --port 5173 &
sleep 3

# Launch desktop
"$DIR/agent-platform/frontend/src-tauri/target/debug/agent-platform"

echo "Agent Platform closed."
