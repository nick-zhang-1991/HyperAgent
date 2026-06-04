#!/usr/bin/env bash
set -euo pipefail

# HyperAgent Release Script
# Builds cross-compiled binaries and creates release tarballs.
#
# Usage:
#   ./scripts/release.sh               # Build for current platform (dist profile)
#   ./scripts/release.sh --all          # Build for all supported targets
#   ./scripts/release.sh --publish v0.2.0  # Tag, build, and push to GitHub
#
# Prerequisites (for cross-compilation):
#   rustup target add x86_64-unknown-linux-gnu
#   rustup target add aarch64-unknown-linux-gnu
#   rustup target add x86_64-apple-darwin
#   rustup target add aarch64-apple-darwin
#   rustup target add x86_64-pc-windows-msvc
#   # Linux ARM cross-compiler:
#   sudo apt install gcc-aarch64-linux-gnu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

VERSION="${GIT_TAG:-$(git describe --tags --always --dirty 2>/dev/null || echo "0.0.0")}"
BINARY="hyperagent"

echo "📦 HyperAgent Release v${VERSION}"
echo "   Project: ${PROJECT_DIR}"
echo ""

build_target() {
    local target="$1"
    local profile="${2:-dist}"
    local suffix="${3:-}"

    echo "🔨 Building for ${target} (profile: ${profile})..."

    local profile_dir
    if [ "$profile" = "dist" ]; then
        profile_dir="dist"
    else
        profile_dir="$profile"
    fi

    cargo build --profile "$profile" --target "$target" 2>&1 | tail -3

    local src="target/${target}/${profile_dir}/${BINARY}${suffix}"
    local dest="target/${target}/${profile_dir}/hyper-${target}${suffix}"

    if [ -f "$src" ]; then
        mv "$src" "$dest"
        echo "   ✅ ${dest}"

        # Strip if not Windows
        if [ "$suffix" != ".exe" ]; then
            strip "$dest" 2>/dev/null || true
        fi

        # Create tarball
        local tarball="hyper-${target}.tar.gz"
        tar czf "target/${tarball}" -C "target/${target}/${profile_dir}" "$(basename "$dest")"
        echo "   📦 target/${tarball} ($(du -h "target/${tarball}" | cut -f1))"
    else
        echo "   ❌ Binary not found at ${src}"
    fi
    echo ""
}

# Default: build for current platform
if [ $# -eq 0 ]; then
    # Detect current target
    case "$(uname -s)" in
        Linux*)  target="x86_64-unknown-linux-gnu" ;;
        Darwin*) target="aarch64-apple-darwin" ;;
        *)       echo "❌ Unknown platform. Build manually."; exit 1 ;;
    esac
    build_target "$target" "dist"
    exit 0
fi

case "${1:-}" in
    --all)
        echo "🚀 Cross-compiling for all targets..."
        build_target "x86_64-unknown-linux-gnu"
        build_target "aarch64-unknown-linux-gnu"
        build_target "x86_64-apple-darwin"
        build_target "aarch64-apple-darwin"
        build_target "x86_64-pc-windows-msvc" "dist" ".exe"
        echo ""
        echo "📂 All artifacts:"
        ls -lh target/hyper-*.tar.gz 2>/dev/null || echo "   (no artifacts found)"
        ;;

    --publish)
        if [ -z "${2:-}" ]; then
            echo "❌ Usage: $0 --publish <tag> (e.g., v0.2.0)"
            exit 1
        fi
        TAG="$2"
        echo "🚀 Publishing ${TAG}..."
        git tag "$TAG"
        git push origin "$TAG"
        echo "   ✅ Tag pushed. GitHub Actions will build and release."
        ;;

    *)
        echo "Usage: $0 [--all|--publish <tag>]"
        exit 1
        ;;
esac
