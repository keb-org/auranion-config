#!/bin/sh
set -e

REPO="keb-org/auranion-config"
INSTALL_DIR="$HOME/.local/bin"

OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" = "Darwin" ]; then
    if [ "$ARCH" = "arm64" ]; then
        ASSET="auranion-macos-arm64"
    else
        ASSET="auranion-macos-amd64"
    fi
elif [ "$OS" = "Linux" ]; then
    ASSET="auranion-linux-amd64"
else
    echo "Unsupported OS: $OS"
    exit 1
fi

mkdir -p "$INSTALL_DIR"

URL="https://github.com/$REPO/releases/latest/download/$ASSET"

echo "Downloading Auranion CLI..."
curl -fsSL "$URL" -o "$INSTALL_DIR/auranion"
chmod +x "$INSTALL_DIR/auranion"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "Please add $INSTALL_DIR to your PATH." ;;
esac

echo "Auranion CLI installed successfully! Run 'auranion config' to start."
