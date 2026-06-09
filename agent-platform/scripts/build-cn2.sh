#!/bin/bash
# Build agent-platform for Linux (CN2 server target)
# Run on macOS: bash scripts/build-cn2.sh

set -e
echo "🔨 Building agent-platform for Linux x86_64..."

# Install cross-compilation target
rustup target add x86_64-unknown-linux-gnu 2>/dev/null || true

# Build backend for Linux
cd agent-platform/backend
cargo build --release --target x86_64-unknown-linux-gnu 2>&1 | tail -3
echo "✅ Backend built: target/x86_64-unknown-linux-gnu/release/agent-platform"

# Build frontend
cd ../frontend
npm run build 2>&1 | tail -3
echo "✅ Frontend built: dist/"

echo ""
echo "Deploy to CN2:"
echo "  scp backend/target/x86_64-unknown-linux-gnu/release/agent-platform root@CN2_IP:/opt/agent-platform/"
echo "  rsync -avz frontend/dist/ root@CN2_IP:/opt/agent-platform/dist/"
echo "  scp scripts/agent-platform.service root@CN2_IP:/etc/systemd/system/"
echo "  scp scripts/nginx-cn2.conf root@CN2_IP:/etc/nginx/sites-enabled/agent-platform"
