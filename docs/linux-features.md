# Linux feature status

The current source targets [Compositor for macOS 1.2.9 (`01e8e52`)](https://github.com/robbietilton/Compositor/tree/01e8e5228af84229923b3a0dc66b40498a3e2dc1), checked September 23, 2026. The published AppImage remains v0.4.0 until these changes are released.

## Added from macOS 1.2.1–1.2.9

| Feature | Linux behavior |
| --- | --- |
| 24 blend modes | Adds Linear Burn, Darker Color, Linear Dodge, Lighter Color, Hard Light, Vivid Light, Linear Light, Pin Light, Hard Mix and Exclusion. Photoshop grouping, CPU/GPU rendering and PSD mappings. |
| Black & White, Color Balance and Invert | Editable adjustment layers and image adjustments, with project persistence and PSD interchange. |
| Gaussian Blur, Motion Blur and Add Noise layers | Editable, masked, blended adjustments over the preceding stack. GPU rendering, chained blur coverage and deterministic noise. |
| Inner Glow | Sixth editable layer effect, with GPU rendering, per-effect visibility, copying and undo. |
| Finishing filters | Vignette (including empty-layer framing), Bloom and Tonal Contrast. |
| Camera Raw Filter | Ten groups for Light, Color, Curves, Color Mixer, Color Grading, Detail, Optics, Geometry, Effects and Calibration. Live preview, graphical curves/wheels, canvas sampling, targeted adjustments, guided geometry and inspection overlays. Portable upstream kernels and GPU blur/geometry. Separate from sensor RAW development. |
| Camera RAW import | Broader camera formats, with Rawler and bundled LibRaw decoding. Editable embedded sources, floating-point development and direct 16-bit TIFF export. See [RAW workflow](linux-raw.md). |
| PSD/PSB import | Large Document import, supported point/paragraph text as editable text, and merged-image fallback for background-only files. Unsupported text retains saved pixels with a conversion report. |
| Project v9 | Reads v1–9 and writes v9, including the new adjustment settings and Inner Glow. |
| Document memory budget | Total raster budget scales from 200 to 800 megapixels with system memory; individual surfaces are limited to 200 megapixels. |
| Layer clipboard and duplication | Copy selected layers/folders within and between tabs, preserving editable content, masks and internal clipping links. Duplicate selected roots together above the topmost source. |
| Crop and Trim | Crop starts at the selection; 3:4 and 9:16 presets. Trim by transparency or either corner color, with independent edge controls and undo. |
| Tool and view behavior | Persistent application-wide toggles, foreground Auto Select, stepped keyboard zoom, expanded context menus, and default automatic update checks, including AppImages. |
| Text interaction | Point-text baseline at the click, Move-tool double-click editing, caret at the end, and live canvas previews from both color pickers. |

Source: [upstream changes since the previous comparison](https://github.com/robbietilton/Compositor/compare/0a73424...01e8e5228af84229923b3a0dc66b40498a3e2dc1). Middle-button panning, stable tool/tab scrolling and titlebar dragging were already implemented in Linux.

## Existing Linux additions

- PSD export, including supported editable adjustments, masks and clipping.
- SAM 3.1 point/box object selection and BiRefNet foreground/background removal, with bundled native Vulkan/CPU inference.
- Open from Clipboard, TIFF/WebP export and direct 16-bit RAW TIFF export.
- Editable RAW source/settings inside `.comp`, local development masks and saved presets.
- Searchable, remappable keyboard shortcuts and Linux desktop integration.

## Compatibility and platform differences

- **Projects:** reads schema versions 1 through 9 and writes version 9 `.comp` directory packages. Editable text, all six layer effects, line shapes, Soft Light and folder opacity are preserved. Unknown project fields are rejected where the schema requires it. Opening Linux-saved projects in the real macOS app remains unverified. See [project I/O](../src/project.rs) and [compatibility tests](../tests/project_compatibility.rs).
- **Background removal:** full BiRefNet Dynamic replaces Apple's proprietary Vision model. Basic/Advanced refinement, editable masks, selection handling and undo are implemented. One AppImage bundles native ONNX Runtime, the WebGPU/Vulkan plugin and both GPU/CPU models. Compatible NVIDIA and AMD Vulkan GPUs need FP16 shader support; no compatible adapter selects CPU. NVIDIA and CPU paths have been tested, physical AMD hardware has not. GPU execution failures report an error rather than silently rerunning on CPU.
- **Rendering:** large brushes, compositing and preview reduction have GPU paths with CPU fallbacks. Tests compare those paths with the Rust CPU reference; they do not establish identical macOS pixels or performance. Individual surfaces are limited to 200 million pixels; total raster storage scales from 200 to 800 million pixels with system memory. Sparse canvases support up to 30,000 pixels per side.
- **Desktop:** Wayland and X11, Ctrl/Alt shortcuts, portal file dialogs and native clipboard integration. Linux window styling differs from AppKit. AppImage updates open GitHub Releases; they do not use Sparkle or replace the mounted executable.
- **Distribution:** x86_64 AppImage, glibc 2.39+, host graphics drivers and desktop portals. Releases are unsigned. No ARM64 AppImage, Flatpak, DEB or RPM is currently provided.

## Photoshop and rendering limits

PSD/PSB import accepts 8-bit RGB and grayscale. Supported primitives and simple point/paragraph text remain editable. Unsupported text, smart objects and some vector content use cached pixels; missing fonts and unsupported styles/transforms are reported. PSD export rasterizes text/shapes, preserves supported adjustment layers, and reports conversions. CMYK and non-8-bit PSD files remain unsupported, as in the original importer. Real macOS and Photoshop application round trips remain unverified.

Bloom approximates Apple's proprietary Core Image filter with Gaussian radiance and screen composition. Background removal uses a different model from Vision. GPU/CPU agreement tests establish consistency within Linux, not pixel-identical output across operating systems. Camera compatibility depends on the bundled decoder versions and the specific sensor/file variant.

## Comparison with Xuan

[Xuan](https://github.com/silverling/xuan), linked in [upstream issue #19](https://github.com/robbietilton/Compositor/issues/19#issuecomment-5744154350), is compared at [`7fd0ee1` (0.2.2)](https://github.com/silverling/xuan/tree/7fd0ee19344c83b586346395a38c9468897c40ec). This is a source/documentation comparison, not a hands-on benchmark.

| Area | This fork | Xuan |
| --- | --- | --- |
| Project format | Reads/writes original `.comp` v9 packages | Imports `.comp` v1–7; saves `.xuan` |
| Background removal | Bundled offline BiRefNet neural segmentation | Border-color matte for simple backgrounds |
| Object selection | SAM 3.1 point/box prompts | Not implemented |
| Imported color profiles | Little CMS conversion to sRGB | Raster ICC profiles are not converted or preserved |
| Layer rendering | 24 blends and six layer effects | 13 blends; attached filter/mask stacks and standalone mask layers |
| Camera RAW | Broader formats through Rawler/LibRaw | Nikon NEF/NRW and Canon CR2/CR3/CRW |
| HEIC import | Bundled libheif decoder | Bundled pure Rust decoder |
| Stroke smoothing | Mouse brush/eraser smoothing | Mouse/pen smoothing; tablet pressure, tilt and eraser-tip support |
| Platforms | Linux AppImage | Linux AppImage, DEB, RPM, archive; Windows ZIP |

Sources: [README](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/README.md), [user guide](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/docs/USAGE.md), [blend modes](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/blend.rs), [document model](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/document.rs), and [filters](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/effects.rs).

## Verification

The integrated ordinary suite passes 797 tests, with 30 hardware or external-fixture tests opt-in. Strict Clippy and the optimized Ubuntu 24.04 build pass. The AppImage payload check verifies bundled dependencies and successfully develops a real Fujifilm X-Pro1 file using its bundled LibRaw. NVIDIA Vulkan checks cover all 24 blend modes, new adjustment layers, chained blurs, Inner Glow and Camera Raw geometry against CPU references. Native X11 checks cover Camera Raw preview, curves, grading, Apply and Undo. Real macOS/Photoshop application round trips and physical AMD GPU testing remain unverified.

The published v0.4.0 AppImage was separately verified before these changes. Its inference models are unchanged. See [model selection and measured results](object-selection-models.md) for inference quality, timing and reproducible checks.
