# Linux architecture and testing

This guide is for contributors. For editing tools and compatibility limits, see [Linux features](linux-features.md). For dependencies and launch instructions, see [Build from source](linux-building.md).

## Architecture

| Component | Responsibility |
| --- | --- |
| [QuickGUI](../vendor/README.md) | Native window, controls, input, clipboard, file dialogs and worker pool. Local patches provide Linux desktop integration. |
| [Document model](../src/document.rs) and [history](../src/session/history_retention.rs) | Layer metadata, shared immutable pixel assets, edit transactions and bounded undo history. |
| [Renderer](../src/render.rs) and [brush engine](../src/brush.rs) | CPU reference implementations and wgpu/Vulkan acceleration for supported workloads. Document and undo assets remain in CPU memory. |
| [Project I/O](../src/project.rs) | Validated `.comp` packages, atomic saves and Linux RAW metadata. See [format reference](project-format.md). |
| [Native inference](../src/inference.rs) | Cached ONNX Runtime sessions with Vulkan GPU or CPU execution. BiRefNet supplies background removal; SAM 3.1 supplies object selection. |
| [Image I/O](../src/image_io.rs) | Image decoding and conversion to sRGB, including libheif and Little CMS integration. |
| [RAW decoding](../src/raw/mod.rs) | Rawler and LibRaw camera support with editable development settings. |

The retained Swift implementation is the macOS behavior reference. Portable C kernels are shared for several image-processing operations. Linux uses its own native window, controls, fonts and graphics backend.

## Verification

Run the [ordinary checks](linux-building.md#checks) before submitting a change. GPU, inference, external image and clipboard tests are opt-in because they need resources outside a source checkout. Run the relevant checks below when modifying those paths.

### GPU rendering and brushes

These tests require a hardware Vulkan adapter:

```sh
cargo test --locked --lib render::gpu::tests -- --ignored --test-threads=1
cargo test --locked --lib brush::coverage::gpu::tests -- --ignored --test-threads=1
cargo test --locked --test brush_parity large_gpu -- --ignored
```

The committed benchmarks report the selected device and workload:

```sh
cargo run --locked --release --example brush_benchmark
cargo run --locked --release --example render_benchmark
cargo run --locked --release --example render_benchmark -- --cpu
```

Their timings exclude native presentation. Use a real desktop session to assess interactive latency.

### Inference and image formats

Prepare the runtime using the [source-build setup](linux-building.md#inference-in-source-builds). Set `COMPOSITOR_BACKGROUND_DEVICE=cpu` or `gpu` to select a backend explicitly. The [object-selection guide](object-selection-models.md#build-and-test) provides the model checks.

```sh
COMPOSITOR_TEST_PHOTO=/path/to/subject-photo.png \
cargo test --locked --test linux_integrations local_background_removal -- --ignored

COMPOSITOR_TEST_HEIC=/path/to/photo.heic \
cargo test --locked --test linux_integrations heic_import -- --ignored

COMPOSITOR_TEST_CMYK=/path/to/profiled-cmyk.tiff \
COMPOSITOR_TEST_CMYK_REFERENCE=/path/to/srgb-reference.png \
cargo test --locked --test linux_integrations cmyk_import -- --ignored
```

Use an independently color-managed reference for the CMYK test. The fixture scripts below download checksum-pinned external samples and print their opt-in test commands:

```sh
scripts/fetch-raw-fixture.sh        # CC0 Nikon D70 NEF
scripts/fetch-raw-fixtures-extra.sh # CC0 Fujifilm X-Pro1 RAF
scripts/fetch-psd-fixtures.sh       # External layered PSD samples
```

### Desktop integration

Clipboard tests replace clipboard contents. Run them on an isolated display:

```sh
# Substitute the display from your isolated X11 session.
env -u WAYLAND_DISPLAY DISPLAY=:94 COMPOSITOR_TEST_CLIPBOARD=x11 \
cargo test --locked --test native_clipboard -- --ignored

# Substitute your isolated Wayland socket and runtime directory.
env -u DISPLAY WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/path/to/isolated/runtime \
COMPOSITOR_TEST_CLIPBOARD=wayland \
cargo test --locked --test native_clipboard -- --ignored
```

The Mutter clipboard check creates its own display and D-Bus session. It requires Mutter and Python GObject bindings:

```sh
cargo build --locked --example wayland_clipboard_check
python3 scripts/check-wayland-clipboard.py
```

Before a release, also exercise the packaged application on Wayland and X11: image import, painting and Undo, save/reopen, file dialogs, clipboard, window resizing and updates. Follow the [AppImage validation instructions](linux-releases.md#validation).

File-format tests and pixel comparisons do not establish identical macOS rendering. See [cross-platform compatibility](macos-compatibility.md) for the tested scope.
