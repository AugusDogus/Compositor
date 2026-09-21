#!/usr/bin/env bash
# Build and test against Ubuntu 24.04's glibc and graphics/image libraries.
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
command -v podman >/dev/null || { printf 'Install Podman to build the Ubuntu 24.04 release baseline.\n' >&2; exit 1; }
rust_sysroot="$(rustc --print sysroot)"
registry_dir="${CARGO_HOME:-$HOME/.cargo}/registry"
# Resolve every locked crate before the container's offline build.
rust_target="$(rustc -vV | sed -n 's/^host: //p')"
cargo fetch --locked --target "$rust_target"
build_dir="$project_root/target/linux-ubuntu24.04"
mkdir -p "$build_dir"
podman build -t localhost/compositor-linux-builder:ubuntu24.04 -f packaging/linux/Containerfile packaging/linux
podman run --rm \
    -v "$project_root:/source:ro" -v "$build_dir:/build" \
    -v "$rust_sysroot:/opt/rust:ro" -v "$registry_dir:/cargo/registry" \
    -e COMPOSITOR_UPDATE_URL \
    localhost/compositor-linux-builder:ubuntu24.04 \
    bash -c 'mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR" && cargo test --locked --offline -- --test-threads=4 && cargo build --locked --offline --release'
COMPOSITOR_LINUX_BINARY="$build_dir/release/compositor" scripts/package-linux.sh
