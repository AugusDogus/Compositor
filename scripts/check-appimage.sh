#!/usr/bin/env bash
# Validate the actual SquashFS payload without requiring FUSE or a desktop session.
set -euo pipefail
[[ $# == 1 ]] || { printf 'Usage: %s path/to/Compositor.AppImage\n' "$0" >&2; exit 1; }
artifact="$(realpath "$1")"
stage_dir="$(mktemp -d)"
trap 'rm -r -- "$stage_dir"' EXIT
cd "$stage_dir"
"$artifact" --appimage-extract > /dev/null
app_dir="$stage_dir/squashfs-root"
for file in AppRun compositor.desktop compositor.png usr/bin/compositor usr/lib/libheif/plugins/libheif-libde265.so; do
    [[ -s "$app_dir/$file" ]] || { printf 'AppImage is missing %s\n' "$file" >&2; exit 1; }
done
sh -n "$app_dir/AppRun"
desktop-file-validate "$app_dir/compositor.desktop"
export COMPOSITOR_INFERENCE_DIR="$app_dir/usr/share/compositor/inference"
export LD_LIBRARY_PATH="$app_dir/usr/lib:$COMPOSITOR_INFERENCE_DIR/lib"
for file in birefnet-cpu.onnx birefnet-gpu.onnx licenses/birefnet.txt lib/libonnxruntime.so lib/libonnxruntime_providers_webgpu.so; do
    [[ -s "$COMPOSITOR_INFERENCE_DIR/$file" ]] || { printf 'AppImage is missing inference dependency %s\n' "$file" >&2; exit 1; }
done
ldd "$app_dir/usr/bin/compositor" > dependencies.txt
ldd "$COMPOSITOR_INFERENCE_DIR/lib/libonnxruntime.so" >> dependencies.txt
ldd "$COMPOSITOR_INFERENCE_DIR/lib/libonnxruntime_providers_webgpu.so" >> dependencies.txt
if grep -q 'not found' dependencies.txt; then cat dependencies.txt >&2; exit 1; fi
if [[ -n "${COMPOSITOR_INFERENCE_TEST_BINARY:-}" ]]; then
    "$COMPOSITOR_INFERENCE_TEST_BINARY" local_background_removal --ignored --nocapture --test-threads=4
fi
printf 'Verified AppImage payload, launcher, icon, HEIC decoder, inference models and native dependencies.\n'
