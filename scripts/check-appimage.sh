#!/usr/bin/env bash
# Validate the actual SquashFS payload without requiring FUSE or a desktop session.
set -euo pipefail
[[ $# == 1 ]] || { printf 'Usage: %s path/to/Compositor.AppImage\n' "$0" >&2; exit 1; }
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
artifact="$(realpath "$1")"
stage_dir="$(mktemp -d)"
trap 'rm -r -- "$stage_dir"' EXIT
cd "$stage_dir"
"$artifact" --appimage-extract > /dev/null
app_dir="$stage_dir/squashfs-root"
if compgen -G "$app_dir/usr/lib/libwayland-client.so*" > /dev/null; then
    printf 'AppImage must use the host Wayland client library required by its graphics drivers.\n' >&2
    exit 1
fi
for file in AppRun compositor.desktop compositor.png usr/bin/compositor usr/lib/libheif/plugins/libheif-libde265.so usr/share/licenses/compositor/Rawler-LGPL-2.1.txt usr/share/licenses/compositor/Rawler-NOTICE.txt usr/share/licenses/compositor/Xuan-MIT.txt; do
    [[ -s "$app_dir/$file" ]] || { printf 'AppImage is missing %s\n' "$file" >&2; exit 1; }
done
sh -n "$app_dir/AppRun"
desktop-file-validate "$app_dir/compositor.desktop"
export COMPOSITOR_INFERENCE_DIR="$app_dir/usr/share/compositor/inference"
export LD_LIBRARY_PATH="$app_dir/usr/lib:$COMPOSITOR_INFERENCE_DIR/lib"
for file in birefnet-cpu.onnx birefnet-gpu.onnx licenses/birefnet.txt lib/libonnxruntime.so lib/libonnxruntime_providers_webgpu.so; do
    [[ -s "$COMPOSITOR_INFERENCE_DIR/$file" ]] || { printf 'AppImage is missing inference dependency %s\n' "$file" >&2; exit 1; }
done
"$script_dir/setup-object-selection.sh" --check
if [[ -n "$(find "$COMPOSITOR_INFERENCE_DIR" -type f \( -name '*.py' -o -name '*.pyc' -o -name '*.whl' \) -print -quit)" ]]; then
    printf 'AppImage inference dependencies must not contain Python code or wheels.\n' >&2
    exit 1
fi
ldd "$app_dir/usr/bin/compositor" > dependencies.txt
ldd "$COMPOSITOR_INFERENCE_DIR/lib/libonnxruntime.so" >> dependencies.txt
ldd "$COMPOSITOR_INFERENCE_DIR/lib/libonnxruntime_providers_webgpu.so" >> dependencies.txt
if grep -q 'not found' dependencies.txt; then cat dependencies.txt >&2; exit 1; fi
if [[ -n "${COMPOSITOR_INFERENCE_TEST_BINARY:-}" ]]; then
    "$COMPOSITOR_INFERENCE_TEST_BINARY" local_background_removal --ignored --nocapture --test-threads=4
fi
if [[ -n "${COMPOSITOR_OBJECT_SELECTION_TEST_BINARY:-}" ]]; then
    if ! "$COMPOSITOR_OBJECT_SELECTION_TEST_BINARY" packaged_object_selection --ignored --list | grep -q ': test$'; then
        printf 'Object-selection test binary has no packaged_object_selection test. Rebuild the library tests from this source revision.\n' >&2
        exit 1
    fi
    "$COMPOSITOR_OBJECT_SELECTION_TEST_BINARY" packaged_object_selection --ignored --nocapture --test-threads=4
fi
printf 'Verified AppImage payload, launcher, icon, HEIC decoder, inference models and native dependencies.\n'
