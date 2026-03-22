#!/usr/bin/env sh
# Waffler CLI installer — Linux & macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/SYW-Apps/waffler-sdk-cli/main/install.sh | sh

set -e

REPO="SYW-Apps/waffler-sdk-cli"
BIN="waffler"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# ─── Detect target ────────────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
      aarch64) TARGET="aarch64-unknown-linux-musl" ;;
      *) echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    EXT="tar.gz"
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-apple-darwin" ;;
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *) echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    EXT="tar.gz"
    ;;
  *)
    echo "Unsupported OS: $OS (use the Windows installer on Windows)"
    exit 1
    ;;
esac

ASSET="${BIN}-${TARGET}.${EXT}"

# ─── Fetch latest release ─────────────────────────────────────────────────────

echo "→ Fetching latest release..."
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\(.*\)".*/\1/')

if [ -z "$TAG" ]; then
  echo "Could not determine latest release tag." && exit 1
fi

echo "→ Installing waffler $TAG ($TARGET)..."

URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
TMP="$(mktemp -d)"

curl -fsSL "$URL" -o "${TMP}/${ASSET}"
tar xzf "${TMP}/${ASSET}" -C "$TMP"

# ─── Install ──────────────────────────────────────────────────────────────────

if [ -w "$INSTALL_DIR" ]; then
  mv "${TMP}/${BIN}" "${INSTALL_DIR}/${BIN}"
else
  sudo mv "${TMP}/${BIN}" "${INSTALL_DIR}/${BIN}"
fi

chmod +x "${INSTALL_DIR}/${BIN}"
rm -rf "$TMP"

echo "✓ waffler $TAG installed to ${INSTALL_DIR}/${BIN}"
echo ""
echo "  Run: waffler --help"
