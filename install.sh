#!/bin/sh
set -e

REPO="keb-org/auranion-config"
INSTALL_DIR="$HOME/.local/bin"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        case "$ARCH" in
            arm64|aarch64)
                ASSET="auranion-macos-arm64"
                ;;
            x86_64)
                if [ "$(sysctl -in sysctl.proc_translated 2>/dev/null)" = "1" ]; then
                    ASSET="auranion-macos-arm64"
                else
                    ASSET="auranion-macos-amd64"
                fi
                ;;
            *)
                echo "Unsupported macOS architecture: $ARCH" >&2
                exit 1
                ;;
        esac
        ;;
    Linux)
        case "$ARCH" in
            x86_64|amd64)
                ASSET="auranion-linux-amd64"
                ;;
            aarch64|arm64)
                ASSET="auranion-linux-arm64"
                ;;
            *)
                echo "Unsupported Linux architecture: $ARCH" >&2
                exit 1
                ;;
        esac
        ;;
    *)
        echo "Unsupported OS: $OS" >&2
        exit 1
        ;;
esac

mkdir -p "$INSTALL_DIR"

URL="https://github.com/$REPO/releases/latest/download/$ASSET"

TMP_FILE="$(mktemp "$INSTALL_DIR/auranion.tmp.XXXXXX")"
trap 'rm -f "$TMP_FILE"' 0
trap 'exit 1' INT TERM

echo "Downloading Auranion CLI..."
curl -fsSL "$URL" -o "$TMP_FILE"

if [ ! -s "$TMP_FILE" ]; then
    echo "Downloaded file is empty." >&2
    exit 1
fi

chmod +x "$TMP_FILE"

if ! "$TMP_FILE" --version >/dev/null 2>&1; then
    echo "Downloaded binary failed execution check." >&2
    exit 1
fi

mv -f "$TMP_FILE" "$INSTALL_DIR/auranion"
trap - 0 INT TERM

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "Please add $INSTALL_DIR to your PATH." ;;
esac

echo "Auranion CLI installed successfully! Run 'auranion config' to start."
