#!/usr/bin/env bash
# Run on Ubuntu 24.04 so bundled libraries retain the documented glibc baseline.
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
[[ "$(uname -m)" == x86_64 ]] || { printf 'AppImage packaging currently supports x86_64.\n' >&2; exit 1; }
for command in curl sha256sum jq cargo file; do
    command -v "$command" >/dev/null || { printf 'Install %s and retry.\n' "$command" >&2; exit 1; }
done
package_binary="${COMPOSITOR_LINUX_BINARY:-target/release/compositor}"
if [[ -z "${COMPOSITOR_LINUX_BINARY:-}" ]]; then cargo build --locked --release; fi
[[ -x "$package_binary" ]] || { printf 'Build the release executable before packaging.\n' >&2; exit 1; }
version="$(cargo metadata --locked --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "compositor") | .version')"
tool_dir="${COMPOSITOR_APPIMAGE_TOOLS:-$project_root/target/appimage-tools}"
mkdir -p "$tool_dir" dist
stage_dir="$(mktemp -d)"
trap 'rm -r -- "$stage_dir"' EXIT
app_dir="$stage_dir/Compositor.AppDir"

download() {
    local name="$1" url="$2" hash="$3"
    if [[ -f "$tool_dir/$name" ]] && printf '%s  %s\n' "$hash" "$tool_dir/$name" | sha256sum --check --status; then return; fi
    curl --fail --location --retry 3 --output "$tool_dir/$name.part" "$url"
    printf '%s  %s\n' "$hash" "$tool_dir/$name.part" | sha256sum --check
    mv "$tool_dir/$name.part" "$tool_dir/$name"
    chmod +x "$tool_dir/$name"
}
download linuxdeploy.AppImage \
    https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage \
    c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
download appimagetool.AppImage \
    https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage \
    ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
download runtime-x86_64 \
    https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 \
    2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

install -Dm755 "$package_binary" "$app_dir/usr/bin/compositor"
install -Dm755 packaging/linux/AppRun "$app_dir/AppRun"
install -Dm644 packaging/linux/compositor.desktop "$app_dir/usr/share/applications/compositor.desktop"
install -Dm644 packaging/linux/io.github.AugusDogus.Compositor.metainfo.xml "$app_dir/usr/share/metainfo/io.github.AugusDogus.Compositor.metainfo.xml"
install -Dm644 Compositor/Assets.xcassets/AppIcon.appiconset/app-icon-256.png "$app_dir/usr/share/icons/hicolor/256x256/apps/compositor.png"
install -Dm644 LICENSE "$app_dir/usr/share/licenses/compositor/LICENSE"
install -Dm644 assets/fonts/INTER-LICENSE.txt "$app_dir/usr/share/licenses/compositor/Inter.txt"
install -Dm644 assets/icons/LICENSE "$app_dir/usr/share/licenses/compositor/Lucide.txt"
for notice in LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md; do
    install -Dm644 "vendor/quickgui/$notice" "$app_dir/usr/share/licenses/compositor/quickgui/$notice"
done

# Winit and wgpu load these libraries dynamically; ldd alone cannot discover them.
libraries=()
for soname in libwayland-cursor.so.0 libwayland-egl.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libX11.so.6 libXcursor.so.1 libXrandr.so.2 libXi.so.6 libvulkan.so.1 libEGL.so.1 libGL.so.1; do
    library="$(ldconfig -p | awk -v name="$soname" '$1 == name && /x86-64/ && !found { print $NF; found=1 }')"
    [[ -n "$library" ]] || { printf 'Install the runtime library %s before packaging.\n' "$soname" >&2; exit 1; }
    libraries+=(--library "$library")
done
# libheif discovers its HEIC decoder at runtime, outside the executable dependency tree.
heif_plugin=/usr/lib/x86_64-linux-gnu/libheif/plugins/libheif-libde265.so
[[ -f "$heif_plugin" ]] || { printf 'Install libheif-plugin-libde265 before packaging.\n' >&2; exit 1; }
install -Dm644 "$heif_plugin" "$app_dir/usr/lib/libheif/plugins/libheif-libde265.so"
libraries+=(--library "$heif_plugin")
export APPIMAGE_EXTRACT_AND_RUN=1
# Host Mesa/NVIDIA EGL drivers can require newer Wayland symbols than the build
# baseline provides. Their matching client library must come from the host too.
NO_STRIP=1 "$tool_dir/linuxdeploy.AppImage" --appdir "$app_dir" \
    --exclude-library 'libwayland-client.so*' \
    --executable "$app_dir/usr/bin/compositor" "${libraries[@]}" \
    --desktop-file "$app_dir/usr/share/applications/compositor.desktop" \
    --icon-file "$app_dir/usr/share/icons/hicolor/256x256/apps/compositor.png"
# Retain distro copyright notices for every potentially bundled runtime dependency.
mkdir -p "$app_dir/usr/share/licenses/system"
while IFS= read -r notice; do
    package="$(basename "$(dirname "$notice")")"
    cp "$notice" "$app_dir/usr/share/licenses/system/$package.txt"
done < <(find /usr/share/doc -maxdepth 2 -name copyright -type f)
# Install checksum-pinned native inference dependencies into the image at build time.
# Keep these separate from linuxdeploy's library rewriting and preserve SONAME links.
COMPOSITOR_INFERENCE_DIR="$app_dir/usr/share/compositor/inference" scripts/setup-background.sh
artifact="$project_root/dist/Compositor-$version-x86_64.AppImage"
ARCH=x86_64 VERSION="$version" "$tool_dir/appimagetool.AppImage" \
    --comp zstd --mksquashfs-opt -Xcompression-level --mksquashfs-opt 19 \
    --mksquashfs-opt -b --mksquashfs-opt 1M \
    --mksquashfs-opt -processors --mksquashfs-opt 2 \
    --runtime-file "$tool_dir/runtime-x86_64" "$app_dir" "$artifact"
if (( $(stat -c %s "$artifact") >= 2147483648 )); then
    printf 'AppImage exceeds GitHub\047s 2 GiB release asset limit. Packaging must be reduced before publishing.\n' >&2
    exit 1
fi
(cd dist && sha256sum "$(basename "$artifact")" > "$(basename "$artifact").sha256")
printf 'Created %s\n' "$artifact"
