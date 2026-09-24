# Build from source

Use Rust 1.94 or newer. The Linux application uses a vendored QuickGUI 0.1.5 with Linux integration fixes. The Swift sources remain the macOS implementation reference.

## Dependencies and launch

On Ubuntu 24.04:

```sh
sudo apt-get install build-essential pkg-config zlib1g-dev libwayland-dev \
  libxkbcommon-dev libxkbcommon-x11-0 libx11-dev libxcb1-dev libx11-xcb-dev \
  libxcursor-dev libxrandr-dev libxi-dev libraw-dev libheif-dev libheif-plugin-libde265 \
  liblcms2-dev libvulkan1 mesa-vulkan-drivers libegl1 libgl1 fonts-dejavu-core
cargo run --locked --release
# Open a project or image:
cargo run --locked --release -- /path/to/project.comp /path/to/photo.jpg
```

Other distributions need equivalent development libraries. Use your desktop's XDG portal backend for native file dialogs and your GPU's graphics drivers. Ubuntu 24.04 is the release build baseline, not the only supported distribution.

## Inference in source builds

The AppImage already includes this runtime. For a source build, prepare the native inference libraries and models once:

```sh
sudo apt-get install curl unzip python3-venv
scripts/setup-linux.sh
scripts/run-linux.sh
```

`setup-linux.sh` runs `setup-background.sh`, then builds the release executable. Setup downloads pinned ONNX Runtime libraries, full BiRefNet Dynamic models, and the Object Selection model. Model preparation uses a build-only Python environment. The editor itself runs native inference and needs no Python runtime. Setup requires network access; subsequent background removal and object selection work offline.

Models and libraries default to `${XDG_DATA_HOME:-$HOME/.local/share}/compositor/inference`. Set `COMPOSITOR_INFERENCE_DIR` during setup and launch to use another directory. `COMPOSITOR_BACKGROUND_DEVICE=cpu` or `gpu` overrides automatic device selection. GPU inference requires a hardware Vulkan adapter with FP16 shader support.

## Checks

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --no-deps -- -D warnings
cargo test --locked -- --test-threads=4
```

Hardware-specific and external-fixture tests are explicitly ignored in the ordinary suite. They require a GPU, inference models or the documented image fixtures. See [implementation and verification](linux-port.md#verification).

For distributable builds, follow [AppImage builds and releases](linux-releases.md).

## RAW decoder and rebuilding

Nikon NEF/NRW decoding uses Rawler 0.7.2, linked into the application under LGPL-2.1. Its source revision is pinned by the crate checksum in `Cargo.lock`; [source and license details](../licenses/Rawler-NOTICE.txt) accompany packaged builds.

To use a modified Rawler, unpack the [exact crate source](https://crates.io/api/v1/crates/rawler/0.7.2/download), make your changes, and add `rawler = { path = "/absolute/path/to/rawler-0.7.2" }` to the existing `[patch.crates-io]` table in `Cargo.toml`. Run `cargo update -p rawler` to record the local override, then `cargo build --release`. This rebuilds and relinks the complete editor with your decoder. The source repository contains the application source, build scripts and dependency lockfile needed for this process.

LibRaw provides native high-precision decoding for X-Trans and other cameras outside Rawler's Bayer path. Source builds require `libraw-dev`; AppImages bundle the library. No runtime setup is needed. Foveon X3F is not supported because unpacking its sensor planes alone does not provide calibrated color development.

To replace LibRaw in an AppImage, extract it and replace `usr/lib/libraw.so*` with an ABI-compatible build. [LibRaw source and license details](../licenses/LibRaw-NOTICE.txt) accompany the application.

Validate the CC0 Fujifilm X-Pro1 fixture with:

```sh
scripts/fetch-raw-fixtures-extra.sh
cargo test --lib raw::libraw::tests::real_xtrans -- --ignored
```
