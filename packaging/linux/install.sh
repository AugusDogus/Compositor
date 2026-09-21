#!/usr/bin/env bash
set -euo pipefail
bundle_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
install_prefix="${1:-$HOME/.local}"
install -Dm755 "$bundle_root/bin/compositor" "$install_prefix/bin/compositor"
install -Dm644 "$bundle_root/share/applications/compositor.desktop" "$install_prefix/share/applications/compositor.desktop"
install -Dm644 "$bundle_root/share/icons/hicolor/256x256/apps/compositor.png" "$install_prefix/share/icons/hicolor/256x256/apps/compositor.png"
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$install_prefix/share/applications"
fi
printf 'Installed Compositor in %s. Add %s/bin to PATH if needed.\n' "$install_prefix" "$install_prefix"
