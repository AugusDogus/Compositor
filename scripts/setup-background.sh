#!/usr/bin/env bash
# Install native ONNX Runtime, CUDA libraries and BiRefNet. No Python or root needed.
set -euo pipefail
[[ "$(uname -sm)" == 'Linux x86_64' ]] || { printf 'Native inference setup currently supports Linux x86_64.\n' >&2; exit 1; }
for command in curl tar sha256sum; do
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

# Hashes from the publishers' GitHub release and NVIDIA redistribution manifests.
runtime=onnxruntime-linux-x64-gpu_cuda12-1.30.0
download "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/$runtime.tgz" \
    f9886932ee7bb0b4d3fcab736a392d4ff5efaa0672b47f19f0cec03437cf64f1 "$cache_dir/$runtime.tgz"
tar -xzf "$cache_dir/$runtime.tgz" -C "$stage_dir"
cp -a "$stage_dir/$runtime/lib/"*.so* "$stage_dir/lib/"
cp "$stage_dir/$runtime/LICENSE" "$inference_dir/licenses/onnxruntime.txt"
if [[ -f "$stage_dir/$runtime/ThirdPartyNotices.txt" ]]; then
    cp "$stage_dir/$runtime/ThirdPartyNotices.txt" "$inference_dir/licenses/onnxruntime-third-party.txt"
fi

install_nvidia() {
    local base="$1" path="$2" checksum="$3" name
    name="$(basename "$path")"
    download "$base/$path" "$checksum" "$cache_dir/$name"
    mkdir -p "$stage_dir/$name"
    tar -xJf "$cache_dir/$name" -C "$stage_dir/$name" --wildcards '*/lib/*.so*' '*/LICENSE'
    cp -a "$stage_dir/$name/"*/lib/*.so* "$stage_dir/lib/"
    cp "$stage_dir/$name/"*/LICENSE "$inference_dir/licenses/$name.txt"
    rm -rf -- "$stage_dir/$name"
}
cuda_url=https://developer.download.nvidia.com/compute/cuda/redist
install_nvidia "$cuda_url" cuda_cudart/linux-x86_64/cuda_cudart-linux-x86_64-12.9.79-archive.tar.xz \
    1f6ad42d4f530b24bfa35894ccf6b7209d2354f59101fd62ec4a6192a184ce99
install_nvidia "$cuda_url" libcublas/linux-x86_64/libcublas-linux-x86_64-12.9.1.4-archive.tar.xz \
    546addc4a9d82b8f23aa9ba9274b6bc0429a63008a31c759884ac24880796057
install_nvidia "$cuda_url" libcurand/linux-x86_64/libcurand-linux-x86_64-10.3.10.19-archive.tar.xz \
    48281b4caadb1cf790d44ac76b23c77d06f474c0b1799814f314aafec9258ad6
install_nvidia https://developer.download.nvidia.com/compute/cudnn/redist \
    cudnn/linux-x86_64/cudnn-linux-x86_64-9.10.2.21_cuda12-archive.tar.xz \
    d0defcbc4c6dad711ff4cb66d254036a300c9071b07c7b64199aacab534313c1

# Full BiRefNet Dynamic, fixed 1024 px input. The CUDA export uses native DeformConv
# and half precision to avoid the older export's large GatherND intermediate tensors.
download https://huggingface.co/onnx-community/BiRefNet_dynamic-1024x1024-ONNX-CUDA/resolve/5e8a5a4aa9b0e0342a22ef6e47e456dd4fd9c4b9/onnx/model.onnx \
    1fefe02158d58b32d589cc4bdd7c2be938cd49171316e4a6ed5da485ccf39b19 "$cache_dir/birefnet-cuda.onnx"
# A full-precision export of the same model supports explicit CPU inference.
download https://huggingface.co/onnx-community/BiRefNet_dynamic-1024x1024-ONNX/resolve/6defd3a19cec21042b178108815c9c4508d09867/onnx/model.onnx \
    aa3c17882a27f079fafa7655cf50e83b9e6e067d6c6fa4cbc3524ae441da0bd7 "$cache_dir/birefnet-cpu.onnx"
download https://raw.githubusercontent.com/ZhengPeng7/BiRefNet/ebcc0bc8ec7fe919cec829f2dea656b3078acddc/LICENSE \
    92a7089e0915fc32bc40067560b398f1e6a7a5958abd7d04eda393629a5acefb "$inference_dir/licenses/birefnet.txt"
for model in birefnet-cuda.onnx birefnet-cpu.onnx; do
    cp --reflink=auto "$cache_dir/$model" "$stage_dir/$model"
    mv -Tf "$stage_dir/$model" "$inference_dir/$model"
done
# Rename complete files so rerunning setup cannot truncate a library in a running editor.
for library in "$stage_dir/lib/"*; do
    mv -Tf "$library" "$inference_dir/lib/$(basename "$library")"
done
printf 'Native CUDA background removal installed in %s. No Python environment is needed.\n' "$inference_dir"
