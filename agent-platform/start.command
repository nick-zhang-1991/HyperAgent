#!/bin/bash
# Agent Platform Launcher — double-click to start
# Starts backend + frontend + desktop together

DIR="$(cd "$(dirname "$0")" && pwd)"

echo "🚀 Starting Agent Platform..."

# Start backend
"$DIR/agent-platform/backend/target/debug/agent-platform" &
BACKEND_PID=$!

# Start frontend  
cd "$DIR/agent-platform/frontend"
./node_modules/.bin/vite --host 0.0.0.0 --port 5173 &
VITE_PID=$!

# Wait for servers
sleep 3

# Launch desktop
"$DIR/agent-platform/frontend/src-tauri/target/debug/agent-platform" &
DESKTOP_PID=$!

echo "Backend:  http://127.0.0.1:4000 (PID $BACKEND_PID)"
echo "Frontend: http://127.0.0.1:5173 (PID $VITE_PID)"
echo "Desktop:  native window (PID $DESKTOP_PID)"

# Keep script alive (so dock shows the app)
wait $DESKTOP_PID
kill $BACKEND_PID $VITE_PID 2>/dev/null
