# Compositor for Linux

A layered image editor for Linux, built with Rust and [QuickGUI](https://github.com/egoist/quickgui). This is a Linux fork of [Robbie Tilton's Compositor](https://github.com/robbietilton/Compositor). The original Swift application remains in this repository as the implementation reference.

## Download

Get the **x86_64 AppImage** from [GitHub Releases](https://github.com/AugusDogus/Compositor/releases), make it executable, and launch it:

```sh
chmod +x Compositor-*-x86_64.AppImage
./Compositor-0.1.0-x86_64.AppImage
```

Requires an x86_64 Linux system with **glibc 2.39 or newer**, a Vulkan or OpenGL driver, and a graphical desktop with XDG portals for native file dialogs. The build baseline is Ubuntu 24.04; other distributions with compatible glibc and graphics drivers also work. Wayland and X11 are supported. The AppImage bundles image codecs and desktop client libraries, while graphics drivers and portal services come from your system.

If FUSE is unavailable, run `./Compositor-0.1.0-x86_64.AppImage --appimage-extract-and-run`. Tools such as [Gear Lever](https://github.com/mijorus/gearlever) can add the AppImage and its icon to your desktop's application menu.

## Features

- Layers, groups, masks, clipping, blend modes and adjustment layers.
- Painting, erasing, clone stamping and healing, with GPU acceleration for large brushes.
- Move, scale, rotate, perspective transforms, shapes, cropping and canvas resizing.
- Rectangle, ellipse, lasso, polygon and magic-wand selections.
- Color adjustments, filters, content-aware fill and local background removal.
- Project saving, recovery, image import/export, color management and native clipboard integration.

Painting, canvas compositing and preview resizing use Vulkan where supported, with CPU fallbacks. The Linux feature inventory is implemented; equivalence with a running macOS app and macOS project round trips are not fully verified. Background removal uses BiRefNet instead of Apple's proprietary Vision model. See [implementation and verification](docs/linux-port.md) for details.

## Background removal

The AppImage bundles ONNX Runtime, CUDA libraries and both full BiRefNet Dynamic models. Background removal works offline immediately, with no dependency installation, Python environment or image uploads.

Inference uses NVIDIA CUDA when the NVIDIA driver is installed, and CPU otherwise. The model stays loaded between removals. GPU inference requires a compatible NVIDIA driver, which comes from your system. `COMPOSITOR_BACKGROUND_DEVICE=cpu` or `cuda` overrides automatic selection; `COMPOSITOR_INFERENCE_DIR` overrides the bundled model/runtime location.

## Build from source

Use Rust 1.94 or newer. On Ubuntu 24.04:

```sh
sudo apt-get install build-essential pkg-config zlib1g-dev libwayland-dev \
  libxkbcommon-dev libxkbcommon-x11-0 libx11-dev libxcb1-dev libx11-xcb-dev \
  libxcursor-dev libxrandr-dev libxi-dev libheif-dev libheif-plugin-libde265 \
  liblcms2-dev libvulkan1 mesa-vulkan-drivers libegl1 libgl1 fonts-dejavu-core
cargo run --locked --release
# Open a project or image:
cargo run --locked --release -- /path/to/project.comp /path/to/photo.jpg
```

`scripts/setup-linux.sh` installs background-removal dependencies and builds the editor. `scripts/run-linux.sh` starts a development build. The application uses a vendored QuickGUI 0.1.5 with Linux integration fixes.

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --no-deps -- -D warnings
cargo test --locked -- --test-threads=4
```

Hardware-specific and external-fixture tests are explicitly ignored in the ordinary suite. They require a GPU, inference models or the documented image fixtures.

## AppImage builds and releases

The [Linux AppImage workflow](.github/workflows/linux-release.yml) tests and packages changes to `main`, pull requests, and manual runs. Build artifacts are downloadable from the Actions run. A pushed `v<VERSION>` tag publishes a GitHub release after validation succeeds. The tag must match `Cargo.toml`.

To package locally on the Ubuntu 24.04 baseline, also install `curl`, `jq`, `file` and `desktop-file-utils`, then run:

```sh
scripts/package-appimage.sh
scripts/prepare-release-assets.sh
scripts/check-appimage.sh dist/*.AppImage
```

The packaging tools and AppImage runtime are pinned and checksum-verified. [Release instructions](docs/linux-releases.md) cover versioning, artifacts and updates. Releases are unsigned, with SHA-256 checksums for download integrity.

## License

MIT, preserving the original Compositor copyright. QuickGUI and bundled dependencies retain their own license notices. See [LICENSE](LICENSE).
