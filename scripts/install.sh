#!/usr/bin/env bash
# HyperAgent — 一键安装脚本
# Usage: curl -fsSL https://raw.githubusercontent.com/YOUR_REPO/hyperagent/main/scripts/install.sh | bash
# Or: ./scripts/install.sh

set -euo pipefail

BIN_DIR="${HOME}/.local/bin"
BUILD_DIR="${HOME}/.local/share/hyperagent"
CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/Library/Application Support}/hyper"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║     HyperAgent 一键安装              ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""

# 1. Detect OS
OS="$(uname -s)"
ARCH="$(uname -m)"
echo -e "${YELLOW}🔍 Detecting system...${NC}"
echo "   OS:   ${OS}"
echo "   Arch: ${ARCH}"

# 2. Check requirements
echo ""
echo -e "${YELLOW}🔧 Checking requirements...${NC}"

if command -v cargo &>/dev/null; then
    RUST_VER=$(rustc --version 2>/dev/null | cut -d' ' -f2)
    echo -e "   ${GREEN}✅ Rust ${RUST_VER}${NC}"
else
    echo -e "   ${RED}❌ Rust not found. Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh${NC}"
    echo "   Then re-run this script."
    exit 1
fi

# 3. Clone or update repo
REPO_DIR="${BUILD_DIR}/repo"
echo ""
echo -e "${YELLOW}📦 Setting up project...${NC}"

if [ -d "${REPO_DIR}" ]; then
    echo "   Found existing repo, updating..."
    cd "${REPO_DIR}"
    git pull --ff-only 2>/dev/null || true
else
    mkdir -p "${BUILD_DIR}"
    echo "   Cloning from current directory..."
    # If running from inside the repo, use that
    if [ -f "./Cargo.toml" ] && grep -q "hyperagent" "./Cargo.toml" 2>/dev/null; then
        REPO_DIR="$(pwd)"
        echo "   Using current directory: ${REPO_DIR}"
    else
        echo "   Need to specify repo URL or run from project root."
        echo "   Usage: cd /path/to/hyperagent && ./scripts/install.sh"
        exit 1
    fi
fi

cd "${REPO_DIR}"

# 4. Build release binary
echo ""
echo -e "${YELLOW}🔨 Building release binary...${NC}"
CARGO_PROFILE_RELEASE_LTO=off cargo build --release 2>&1 | tail -1
echo -e "   ${GREEN}✅ Build complete${NC}"

# 5. Install binary
echo ""
echo -e "${YELLOW}📋 Installing binary...${NC}"
mkdir -p "${BIN_DIR}"
cp "target/release/hyperagent" "${BIN_DIR}/hyper"
echo -e "   ${GREEN}✅ Installed to ${BIN_DIR}/hyper${NC}"

# 6. Verify PATH
if [[ ":$PATH:" != *":${BIN_DIR}:"* ]]; then
    echo -e "   ${YELLOW}⚠️  ${BIN_DIR} not in PATH. Add to ~/.zshrc:${NC}"
    echo "      export PATH=\"${BIN_DIR}:\$PATH\""
fi

# 7. Create default config if needed
echo ""
echo -e "${YELLOW}⚙️  Checking config...${NC}"
if [ ! -f "${CONFIG_DIR}/config.toml" ]; then
    mkdir -p "${CONFIG_DIR}"
    cp "config/default.toml" "${CONFIG_DIR}/config.toml" 2>/dev/null || {
        echo "   Creating minimal config..."
        cat > "${CONFIG_DIR}/config.toml" << 'EOF'
[[providers]]
name = "deepseek"
api_key = ""
base_url = "https://api.deepseek.com/v1"
default_model = "deepseek-v4-flash"
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
priority = 1
weight = 1.0

[[agents]]
name = "build"
mode = "Primary"
model = "deepseek-v4-flash"
temperature = 0.1
description = "Execute code modifications"
[agents.permissions]
edit = "Allow"
bash = "Allow"
read = "Allow"
network = "Deny"
EOF
    }
    echo -e "   ${GREEN}✅ Default config created${NC}"
else
    echo -e "   ${GREEN}✅ Config exists at ${CONFIG_DIR}/config.toml${NC}"
fi

# 8. Verify installation
echo ""
echo -e "${YELLOW}🧪 Verifying installation...${NC}"
if "${BIN_DIR}/hyper" doctor 2>&1 | grep -q "System Diagnostics"; then
    echo -e "   ${GREEN}✅ HyperAgent installed successfully!${NC}"
else
    echo -e "   ${RED}❌ Installation verification failed${NC}"
    exit 1
fi

echo ""
echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║     HyperAgent 安装完成               ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""
echo "   Quick start:"
echo "     hyper init              # Build index in current project"
echo "     hyper run \"fix bug\"      # Run coding task"
echo "     hyper run \"explain\" --mode ask  # Ask about code"
echo "     hyper doctor            # Diagnostics"
echo ""
echo "   Need API keys? Set env vars:"
echo "     export DEEPSEEK_API_KEY=\"sk-...\""
echo "   Or edit: ${CONFIG_DIR}/config.toml"
echo ""
