#!/usr/bin/env bash
# Install United from a release build. Works on any distro: it only copies a
# binary, a desktop entry and an icon, then refreshes the desktop cache when the
# helper exists. No package manager, no root unless PREFIX=/usr.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$here/.."
bin="$root/target/release/united"
prefix="${PREFIX:-$HOME/.local}"
appdir="$prefix/share/applications"

[[ -x "$bin" ]] || { echo "build first: cargo build --release -p united-app" >&2; exit 1; }

install -Dm755 "$bin" "$prefix/bin/united"
install -Dm644 "$here/united.desktop" "$appdir/united.desktop"
install -Dm644 "$here/united-256.png" "$prefix/share/icons/hicolor/256x256/apps/united.png"
install -Dm644 "$here/united-512.png" "$prefix/share/icons/hicolor/512x512/apps/united.png"
# The old placeholder shipped as a scalable SVG and would win the theme lookup.
rm -f "$prefix/share/icons/hicolor/scalable/apps/united.svg"

command -v update-desktop-database >/dev/null && update-desktop-database "$appdir" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$prefix/share/icons/hicolor" 2>/dev/null || true

echo "installed: $prefix/bin/united"
