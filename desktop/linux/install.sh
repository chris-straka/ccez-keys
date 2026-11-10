#!/bin/sh
# Puts ccez-keys in ~/.local (no root): the app, its settings in the app list,
# its icon, and starts it. Accessibility must be on (GNOME: it is, for AT-SPI apps).
set -e
cd "$(dirname "$0")"
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/64x64/apps
install -m 755 ccez-keys-desktop ~/.local/bin/
install -m 644 ccez-keys.desktop ccez-keys-settings.desktop ~/.local/share/applications/
install -m 644 icon.png ~/.local/share/icons/hicolor/64x64/apps/ccez-keys.png
pkill -x ccez-keys-desktop 2>/dev/null || true
nohup ~/.local/bin/ccez-keys-desktop >/dev/null 2>&1 &
echo "ccez-keys installed and started"
