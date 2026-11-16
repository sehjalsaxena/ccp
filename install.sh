#!/usr/bin/env sh
set -e

REPO="${CCP_REPO:-smresponsibilities/ccp}"

# Detect Operating System (macOS, Linux, WSL)
OS="$(uname -s)"
case "$OS" in
  Linux)
    OS="linux"
    ;;
  Darwin)
    OS="darwin"
    ;;
  *)
    echo "❌ Unsupported operating system: $OS"
    exit 1
    ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)
    ARCH="amd64"
    ;;
  aarch64|arm64)
    ARCH="arm64"
    ;;
  *)
    echo "❌ Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

ASSET_NAME="ccp-${OS}-${ARCH}.tar.gz"

if [ -n "$CCP_VERSION" ]; then
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${CCP_VERSION}/${ASSET_NAME}"
else
  DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET_NAME}"
fi

# Determine install location (default: ~/.local/bin)
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "==> Installing ccp (${OS}/${ARCH})..."
echo "==> Downloading from: $DOWNLOAD_URL"

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'ccp')"
trap 'rm -rf "$TMP_DIR"' EXIT

if ! curl -fSL "$DOWNLOAD_URL" -o "$TMP_DIR/$ASSET_NAME"; then
  echo "❌ Failed to download $ASSET_NAME."
  echo "Please check if a release exists at https://github.com/${REPO}/releases"
  exit 1
fi

tar -xzf "$TMP_DIR/$ASSET_NAME" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
mv "$TMP_DIR/ccp" "$INSTALL_DIR/ccp"
chmod +x "$INSTALL_DIR/ccp"

echo "✅ ccp installed to $INSTALL_DIR/ccp"

# Check if INSTALL_DIR is in PATH
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "⚠️  $INSTALL_DIR is not in your PATH."
    echo "Add it by running:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo "Or append it to your ~/.bashrc or ~/.zshrc."
    ;;
esac

echo ""
echo "Run 'ccp set' to configure credentials or 'ccp' to log in."
