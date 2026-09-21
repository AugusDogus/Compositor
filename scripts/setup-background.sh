#!/usr/bin/env bash
# Prepare bundled native Vulkan/CPU inference. Python is used only to build the model.
set -euo pipefail
[[ "$(uname -sm)" == 'Linux x86_64' ]] || { printf 'Native inference setup currently supports Linux x86_64.\n' >&2; exit 1; }
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
for command in curl tar unzip sha256sum python3; do
    command -v "$command" >/dev/null || { printf 'Install %s and retry.\n' "$command" >&2; exit 1; }
done
inference_dir="${COMPOSITOR_INFERENCE_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/compositor/inference}"
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/compositor/inference-downloads"
mkdir -p "$inference_dir/lib" "$inference_dir/licenses" "$cache_dir"
stage_dir="$(mktemp -d "$inference_dir/.setup.XXXXXX")"
trap 'rm -rf -- "$stage_dir"' EXIT
mkdir -p "$stage_dir/lib"

download() {
    local url="$1" checksum="$2" destination="$3"
    if [[ -f "$destination" ]] && printf '%s  %s\n' "$checksum" "$destination" | sha256sum --check --status; then
        return
    fi
    curl --fail --location --retry 3 --output "$destination.part" "$url"
    printf '%s  %s\n' "$checksum" "$destination.part" | sha256sum --check
    mv "$destination.part" "$destination"
}

# Native CPU core plus the vendor-independent Vulkan plugin. The wheel is only a
# distribution archive: extract its native library and licenses, never Python code.
runtime=onnxruntime-linux-x64-1.30.0
download "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/$runtime.tgz" \
    a5ed5a3cac51fbb2e90da632ae43d19212faaa20e76484e62bcb7c23ddb3b3fd "$cache_dir/$runtime.tgz"
tar -xzf "$cache_dir/$runtime.tgz" -C "$stage_dir"
cp -a "$stage_dir/$runtime/lib/"*.so* "$stage_dir/lib/"
cp "$stage_dir/$runtime/LICENSE" "$inference_dir/licenses/onnxruntime.txt"
cp "$stage_dir/$runtime/ThirdPartyNotices.txt" "$inference_dir/licenses/onnxruntime-third-party.txt"
plugin=onnxruntime_ep_webgpu-0.3.0.whl
download https://files.pythonhosted.org/packages/97/9c/d37bc05c56c3d91d44585db7bebbf0f068ece5d01df5b3898449771d4bf2/onnxruntime_ep_webgpu-0.3.0-py3-none-manylinux_2_28_x86_64.whl \
    865ce82d80319d7f259a4a65e66e32834f0f117db55ae4377868b4f28016e7bf "$cache_dir/$plugin"
unzip -p "$cache_dir/$plugin" onnxruntime_ep_webgpu/libonnxruntime_providers_webgpu.so > "$stage_dir/lib/libonnxruntime_providers_webgpu.so"
chmod 755 "$stage_dir/lib/libonnxruntime_providers_webgpu.so"
unzip -p "$cache_dir/$plugin" onnxruntime_ep_webgpu-0.3.0.dist-info/licenses/LICENSE > "$inference_dir/licenses/webgpu.txt"
unzip -p "$cache_dir/$plugin" onnxruntime_ep_webgpu-0.3.0.dist-info/licenses/ThirdPartyNotices.txt > "$inference_dir/licenses/webgpu-third-party.txt"

# Full BiRefNet Dynamic, fixed 1024 px input. The CUDA export uses native DeformConv
# and half precision. Lower DeformConv to equivalent Vulkan-supported operations.
download https://huggingface.co/onnx-community/BiRefNet_dynamic-1024x1024-ONNX-CUDA/resolve/5e8a5a4aa9b0e0342a22ef6e47e456dd4fd9c4b9/onnx/model.onnx \
    1fefe02158d58b32d589cc4bdd7c2be938cd49171316e4a6ed5da485ccf39b19 "$cache_dir/birefnet-cuda.onnx"
# A full-precision export of the same model supports explicit CPU inference.
download https://huggingface.co/onnx-community/BiRefNet_dynamic-1024x1024-ONNX/resolve/6defd3a19cec21042b178108815c9c4508d09867/onnx/model.onnx \
    aa3c17882a27f079fafa7655cf50e83b9e6e067d6c6fa4cbc3524ae441da0bd7 "$cache_dir/birefnet-cpu.onnx"
download https://raw.githubusercontent.com/ZhengPeng7/BiRefNet/ebcc0bc8ec7fe919cec829f2dea656b3078acddc/LICENSE \
    92a7089e0915fc32bc40067560b398f1e6a7a5958abd7d04eda393629a5acefb "$inference_dir/licenses/birefnet.txt"
# Pin the transformed output as well as the source. Rebuild only when its checksum
# changes, and test the lowering against native DeformConv before accepting it.
gpu_hash=30b670d9a05f0c8da5faa689ba9ea061696883c2a30559cbc75fbba30e2ba592
gpu_model="$cache_dir/birefnet-gpu-$gpu_hash.onnx"
if ! { [[ -f "$gpu_model" ]] && printf '%s  %s\n' "$gpu_hash" "$gpu_model" | sha256sum --check --status; }; then
    build_env="$cache_dir/model-build-env"
    if [[ ! -x "$build_env/bin/python" ]]; then python3 -m venv "$build_env"; fi
    "$build_env/bin/python" -m pip install --disable-pip-version-check -r "$script_dir/requirements-inference-build.txt"
    "$build_env/bin/python" -B "$script_dir/test_background_model.py"
    "$build_env/bin/python" -B "$script_dir/background_model.py" "$cache_dir/birefnet-cuda.onnx" "$stage_dir/birefnet-gpu.onnx"
    printf '%s  %s\n' "$gpu_hash" "$stage_dir/birefnet-gpu.onnx" | sha256sum --check
    mv "$stage_dir/birefnet-gpu.onnx" "$gpu_model"
fi
cp --reflink=auto "$gpu_model" "$stage_dir/birefnet-gpu.onnx"
cp --reflink=auto "$cache_dir/birefnet-cpu.onnx" "$stage_dir/birefnet-cpu.onnx"
for model in birefnet-gpu.onnx birefnet-cpu.onnx; do
    mv -Tf "$stage_dir/$model" "$inference_dir/$model"
done
# Rename complete files so rerunning setup cannot truncate a library in a running editor.
for library in "$stage_dir/lib/"*; do
    mv -Tf "$library" "$inference_dir/lib/$(basename "$library")"
done
printf 'Native Vulkan and CPU background removal installed in %s. The application needs no Python runtime.\n' "$inference_dir"
