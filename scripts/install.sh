#!/usr/bin/env bash
# HyperAgent — 一键安装脚本 (supports pre-built binary & source build)
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/.../install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/.../install.sh | bash -s -- --version v1.0.0
#   curl -fsSL https://raw.githubusercontent.com/.../install.sh | bash -s -- --build

set -euo pipefail

# ============================================================================
# Configuration
# ============================================================================
BIN_DIR="${HOME}/.local/bin"
VERSION="${VERSION:-latest}"
INSTALL_MODE="${INSTALL_MODE:-auto}"   # auto, binary, source
REPO="hyperagent"
OWNER="${OWNER:-nick-zhang-1991}"  # GitHub owner/org

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ============================================================================
# Parse args
# ============================================================================
while [[ $# -gt 0 ]]; do
    case $1 in
        --version) VERSION="$2"; shift 2 ;;
        --build) INSTALL_MODE="source"; shift ;;
        --binary) INSTALL_MODE="binary"; shift ;;
        --help) echo "Usage: install.sh [--version vX.Y.Z] [--build|--binary]"; exit 0 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║     HyperAgent Installer             ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""

# ============================================================================
# Detect platform
# ============================================================================
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "${ARCH}" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) echo -e "${RED}Unsupported architecture: ${ARCH}${NC}"; exit 1 ;;
esac

case "${OS}" in
    linux)   TARGET="${ARCH}-unknown-linux-gnu" ;;
    darwin)  TARGET="${ARCH}-apple-darwin" ;;
    mingw*)  echo -e "${YELLOW}Windows detected! Use install.ps1 instead:${NC}"
             echo "   powershell -ExecutionPolicy Bypass -File scripts/install.ps1"
             exit 0 ;;
    *)       echo -e "${RED}Unsupported OS: ${OS}${NC}"
             echo "   For Windows, use: scripts/install.ps1"
             exit 1 ;;
esac

echo -e "${YELLOW}🔍 Detected: ${OS} / ${ARCH} (${TARGET})${NC}"

# ============================================================================
# Determine install mode
# ============================================================================
BINARY_AVAILABLE=false
if command -v curl &>/dev/null; then
    # Check if GitHub release exists for this version
    if [[ "${VERSION}" == "latest" ]]; then
        BINARY_AVAILABLE=true  # Will try download, fallback to build
    else
        BINARY_AVAILABLE=true
    fi
fi

if [[ "${INSTALL_MODE}" == "auto" ]] && [[ "${BINARY_AVAILABLE}" == "true" ]]; then
    INSTALL_MODE="binary"
elif [[ "${INSTALL_MODE}" == "auto" ]]; then
    INSTALL_MODE="source"
fi

# ============================================================================
# Install via pre-built binary
# ============================================================================
if [[ "${INSTALL_MODE}" == "binary" ]]; then
    echo -e "${YELLOW}📦 Downloading pre-built binary...${NC}"

    if [[ "${VERSION}" == "latest" ]]; then
        DOWNLOAD_URL="https://github.com/${OWNER}/${REPO}/releases/latest/download/${REPO}-${TARGET}.tar.gz"
    else
        DOWNLOAD_URL="https://github.com/${OWNER}/${REPO}/releases/download/${VERSION}/${REPO}-${TARGET}.tar.gz"
    fi

    TMP_DIR=$(mktemp -d)
    TAR_FILE="${TMP_DIR}/hyperagent.tar.gz"

    if curl -fsSL "${DOWNLOAD_URL}" -o "${TAR_FILE}" 2>/dev/null; then
        echo -e "   ${GREEN}✅ Downloaded binary${NC}"
        mkdir -p "${BIN_DIR}"
        tar xzf "${TAR_FILE}" -C "${TMP_DIR}" 2>/dev/null || {
            # Try different tar layout
            cp "${TAR_FILE}" "${BIN_DIR}/hyperagent.tar.gz"
            cd "${BIN_DIR}" && tar xzf hyperagent.tar.gz 2>/dev/null && rm hyperagent.tar.gz
        }

        # Find the hyperagent binary in extracted files
        if [[ -f "${TMP_DIR}/hyperagent" ]]; then
            cp "${TMP_DIR}/hyperagent" "${BIN_DIR}/hyper"
        elif [[ -f "${TMP_DIR}/release/hyperagent" ]]; then
            cp "${TMP_DIR}/release/hyperagent" "${BIN_DIR}/hyper"
        elif compgen -G "${TMP_DIR}/*/hyperagent" > /dev/null; then
            cp "${TMP_DIR}"/*/hyperagent "${BIN_DIR}/hyper"
        else
            echo -e "${YELLOW}   ⚠️  Binary layout unknown, falling back to source build${NC}"
            INSTALL_MODE="source"
        fi

        rm -rf "${TMP_DIR}"
    else
        echo -e "${YELLOW}   ⚠️  Binary download failed (no release yet?), falling back to source build${NC}"
        INSTALL_MODE="source"
    fi
fi

# ============================================================================
# Install via source build
# ============================================================================
if [[ "${INSTALL_MODE}" == "source" ]]; then
    echo -e "${YELLOW}🔧 Building from source...${NC}"

    # Check Rust
    if ! command -v rustc &>/dev/null; then
        echo -e "${RED}❌ Rust not found.${NC}"
        echo "   Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd 2>/dev/null || echo "")"

    if [[ -z "${PROJECT_DIR}" ]] || [[ ! -f "${PROJECT_DIR}/Cargo.toml" ]]; then
        echo -e "${RED}❌ Must run install.sh from the hyperagent project root.${NC}"
        echo "   cd /path/to/hyperagent && ./scripts/install.sh"
        exit 1
    fi

    cd "${PROJECT_DIR}"
    echo "   Building release binary (this may take a few minutes)..."
    cargo build --release 2>&1 | tail -1
    echo -e "   ${GREEN}✅ Build complete${NC}"

    mkdir -p "${BIN_DIR}"
    cp "target/release/hyperagent" "${BIN_DIR}/hyper"
fi

# ============================================================================
# Verify installation
# ============================================================================
echo ""
echo -e "${YELLOW}🧪 Verifying installation...${NC}"
chmod +x "${BIN_DIR}/hyper"

if "${BIN_DIR}/hyper" --version &>/dev/null; then
    VERSION_OUTPUT=$("${BIN_DIR}/hyper" --version 2>/dev/null || true)
    echo -e "   ${GREEN}✅ Installed: ${VERSION_OUTPUT}${NC}"
else
    echo -e "${RED}❌ Installation verification failed${NC}"
    exit 1
fi

# PATH check
if [[ ":$PATH:" != *":${BIN_DIR}:"* ]]; then
    echo -e "${YELLOW}   ⚠️  ${BIN_DIR} not in PATH. Add to shell config:${NC}"
    echo "      export PATH=\"${BIN_DIR}:\$PATH\""
fi

# Config check
CONFIG_DIR="${HOME}/Library/Application Support/hyper"
if [[ "${OS}" == "linux" ]]; then
    CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/hyper"
fi

if [[ ! -f "${CONFIG_DIR}/config.toml" ]]; then
    echo ""
    echo -e "${YELLOW}⚙️  First-time setup:${NC}"
    echo "   Run: hyper config-init"
    echo "   Then set API key in: ${CONFIG_DIR}/config.toml"
    echo "   Or use env vars: export HYPER_LLM_API_KEY=\"sk-...\""
fi

echo ""
echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║     HyperAgent Ready! 🚀             ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""
echo "   Quick start:"
echo "     cd your-project"
echo "     hyper init"
echo "     hyper run \"add error handling\""
echo "     hyper run \"explain this\" --mode ask"
echo "     hyper diff --side-by-side"
echo ""
