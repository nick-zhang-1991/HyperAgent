#!/bin/bash
# deploy-cn2.sh — Deploy agent-platform to CN2 server
# Usage: bash scripts/deploy-cn2.sh <CN2_SERVER_IP>

CN2=${1:?"Usage: bash deploy-cn2.sh <CN2_IP>"}
echo "🚀 Deploying to $CN2..."

# 1. Build
bash scripts/build-cn2.sh || { echo "Build failed"; exit 1; }

# 2. Upload
echo "📦 Uploading..."
ssh root@$CN2 "mkdir -p /opt/agent-platform /opt/agent-platform/dist"
scp backend/target/x86_64-unknown-linux-gnu/release/agent-platform root@$CN2:/opt/agent-platform/
rsync -avz --delete frontend/dist/ root@$CN2:/opt/agent-platform/dist/
scp scripts/agent-platform.service root@$CN2:/etc/systemd/system/
scp scripts/nginx-cn2.conf root@$CN2:/etc/nginx/sites-enabled/agent-platform

# 3. Start services
echo "▶️ Starting services..."
ssh root@$CN2 "
    systemctl daemon-reload
    systemctl enable agent-platform
    systemctl restart agent-platform
    systemctl reload nginx
"

# 4. Verify
sleep 2
echo "🔍 Verifying..."
curl -s "http://$CN2/api/orgs" | head -1

echo "✅ Deployed!"
echo "   API:  http://$CN2/api/"
echo "   Web:  http://$CN2/"
