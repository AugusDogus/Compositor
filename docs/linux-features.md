# Linux feature status

Compared on September 20, 2026. This Linux implementation is based on [macOS 1.0.4 (`a19db90`)](https://github.com/robbietilton/Compositor/tree/a19db9011282399785dc18efcfded904627bdcc2). The upstream version checked here is [1.1.6 (`9d5582d`)](https://github.com/robbietilton/Compositor/tree/9d5582dc59429501e270828b27879de9ca30a853).

The 1.0.4 editing inventory has been implemented and exercised on Linux. This is implementation coverage, not a claim of 100% equivalence with current macOS. The [README](../README.md#features-and-parity) lists the available tools; the [acceptance inventory](linux-port.md#parity-acceptance-and-platform-differences) records their verification.

## Newer macOS features not yet ported

| Feature | Current Linux behavior | Source evidence |
| --- | --- | --- |
| Editable text | No Type tool or editable text-layer metadata | [`LayerContent`](../src/document.rs), [upstream Type tool](https://github.com/robbietilton/Compositor/blob/9d5582dc59429501e270828b27879de9ca30a853/Compositor/Document/TypeTool.swift) |
| Layer effects | No editable stroke, shadow, overlay or other layer-effect stack | [`Layer`](../src/document.rs), [upstream effects](https://github.com/robbietilton/Compositor/blob/9d5582dc59429501e270828b27879de9ca30a853/Compositor/Document/LayerEffects.swift) |
| Line shapes | Rectangle, rounded rectangle and ellipse shapes only; straight brush strokes are available | [`ShapeKind`](../src/document.rs) |
| Selection feathering | Antialiasing and expand/contract are available; no Feather command | [`Selection`](../src/selection.rs), [upstream addition](https://github.com/robbietilton/Compositor/commit/2e8ebd7) |
| Soft Light | The original 13 blend modes are available | [`Blend`](../src/blend.rs), [upstream addition](https://github.com/robbietilton/Compositor/commit/e521ca0) |
| Folder duplication | Duplicate Layer excludes groups | [`duplicate_active`](../src/layer_ops.rs), [upstream addition](https://github.com/robbietilton/Compositor/commit/9f49d67) |
| Folder opacity | Groups must retain full opacity and Normal blend mode; masks remain available | [Document validation](../src/document.rs), [upstream addition](https://github.com/robbietilton/Compositor/commit/391042d) |
| Keyboard-shortcuts window | Commands have shortcuts and menu hints, but no dedicated reference window | [Menus](../src/ui/menus/entries.rs), [upstream addition](https://github.com/robbietilton/Compositor/commit/391042d) |

These are identified feature gaps in the newer upstream changes. Smaller behavior and UI refinements in those commits have not all been reconciled.

## Compatibility and platform differences

- **Projects:** reads schema versions 1 through 7 and writes version 7 `.comp` directory packages. Unknown metadata is rejected to avoid silently losing edits. Newer macOS projects containing text, effects, line shapes, Soft Light or folder opacity can therefore fail to open even when their format version is still 7. Opening Linux-saved projects in the real macOS app remains unverified. See [project I/O](../src/project.rs) and [compatibility tests](../tests/project_compatibility.rs).
- **Background removal:** full BiRefNet Dynamic replaces Apple's proprietary Vision model. Basic/Advanced refinement, editable masks, selection handling and undo are implemented. One AppImage bundles native ONNX Runtime, the WebGPU/Vulkan plugin and both GPU/CPU models. Compatible NVIDIA and AMD Vulkan GPUs need FP16 shader support; no compatible adapter selects CPU. NVIDIA and CPU paths have been tested, physical AMD hardware has not. GPU execution failures report an error rather than silently rerunning on CPU.
- **Rendering:** large brushes, compositing and preview reduction have GPU paths with CPU fallbacks. Tests compare those paths with the Rust CPU reference; they do not establish identical macOS pixels or performance. The raster budget is 100 million pixels, and sparse canvases support up to 30,000 pixels per side.
- **Desktop:** Wayland and X11, Ctrl/Alt shortcuts, portal file dialogs and native clipboard integration. Linux window styling differs from AppKit. AppImage updates open GitHub Releases; they do not use Sparkle or replace the mounted executable.
- **Distribution:** x86_64 AppImage, glibc 2.39+, host graphics drivers and desktop portals. Releases are unsigned. No ARM64 AppImage, Flatpak, DEB or RPM is currently provided.

## Comparison with Xuan

[Xuan](https://github.com/silverling/xuan) is the Linux port linked in [upstream issue #19](https://github.com/robbietilton/Compositor/issues/19#issuecomment-5744154350). The comparison below uses its [README](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/README.md) and [user guide](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/docs/USAGE.md) at `0653436`, not a hands-on benchmark or an audit of every feature.

| Area | This fork | Xuan's documented behavior |
| --- | --- | --- |
| Project format | Reads and writes original `.comp` packages, with the limits above | Imports `.comp` v1–7; saves `.xuan` |
| Background removal | Bundled offline BiRefNet neural segmentation, Vulkan/CPU inference | Border-color matte for simple backgrounds |
| Healing and content-aware fill | Reuses upstream's portable C kernels | Portable texture-matching implementation with differing results |
| Imported color profiles | Little CMS conversion to sRGB, including RGB, grayscale, CMYK and Lab | Imported raster ICC profiles are not converted or preserved |
| Text and RAW | No editable text or dedicated RAW development | Editable text; Nikon NEF/NRW development |
| Image export | PNG and JPEG | PNG, JPEG, TIFF and WebP; direct 16-bit TIFF from RAW Develop |
| HEIC import | libheif decoder bundled in the AppImage | Optional external `heif-convert` |
| Distribution | One AppImage with background-removal runtime/models included | DEB, RPM and portable archive |

Both projects provide layered editing, masks, transforms, selections, retouching, adjustments, and Wayland/X11 support. This fork emphasizes the original project format and processing behavior, color-managed imports, and offline neural background removal. Xuan covers workflows this fork does not, especially text and RAW. Neither this table nor a test count establishes an overall winner.
