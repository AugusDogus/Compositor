#!/usr/bin/env bash
# Prepare release metadata beside an already built AppImage. Does not publish.
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
version="$(cargo metadata --locked --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "compositor") | .version')"
[[ -f "dist/Compositor-$version-x86_64.AppImage" ]] || { printf 'Build the AppImage first.\n' >&2; exit 1; }
executable="Compositor-$version-linux-x86_64.bin"
install -m755 "${COMPOSITOR_LINUX_BINARY:-target/release/compositor}" "dist/$executable"
(cd dist && sha256sum "$executable" > "$executable.sha256")
jq -n --arg version "$version" --arg date "$(date -u +%FT%TZ)" \
    --arg url "https://github.com/AugusDogus/Compositor/releases/download/v$version/$executable" \
    '{version:$version,pub_date:$date,notes:("Compositor "+$version+" for Linux."),platforms:{"linux-x86_64":{url:$url}}}' > dist/linux-update.json
cat > dist/release-notes.md <<'NOTES'
A Linux fork of Compositor built with Rust and QuickGUI.

Download the **x86_64 AppImage**, make it executable, and run it. Requires glibc 2.39 or newer, a graphical desktop with XDG portals, and a working Vulkan or OpenGL driver. If FUSE is unavailable, run with `--appimage-extract-and-run`.

Background removal works offline immediately: native ONNX Runtime, CUDA libraries and both full BiRefNet Dynamic models are bundled. No Python or dependency installation is required. NVIDIA CUDA is selected when its driver is installed; other machines use CPU inference. GPU inference requires a compatible NVIDIA driver.

The `.bin` and `linux-update.json` assets support existing standalone executable installations. AppImage users update by downloading the new AppImage and replacing the previous one after closing the editor. SHA-256 files verify download integrity; these releases are unsigned.
NOTES
