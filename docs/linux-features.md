# Linux feature status

The comparison targets [Compositor for macOS 1.2.0 (`28855e6`)](https://github.com/robbietilton/Compositor/tree/28855e684d0b99dd23f6505af718a342cb3af3d2), checked on September 21, 2026. Linux feature status is for v0.3.0.

## Changes in macOS 1.2.0

| Upstream change | Linux v0.3.0 status |
| --- | --- |
| PSD import | Already supported, along with PSD export. macOS imports 8-bit RGB; Linux also accepts 8-bit grayscale. Both preserve supported shapes and adjustments and report conversions. |
| Outer Glow | Missing. The four existing effects remain supported; importing and resaving a project drops its unrecognized `outerGlow` metadata. |
| Brush smoothing | Missing. Upstream adds an adjustable pointer-following distance for Paint and Erase, with catch-up on release. This differs from object-selection outline smoothing. |
| Layer-effect persistence and visibility fixes | Linux already saves supported effects and their visibility, and filters hidden effects independently. |
| Folder-opacity save fix | Linux saves folder opacity, but writes v7 when there are no guides. macOS 1.2.0 requires v8 for non-default folder opacity, so it rejects those Linux packages. |

Sources: [release notes](https://github.com/robbietilton/Compositor/releases/tag/v1.2.0), [PSD importer](https://github.com/robbietilton/Compositor/tree/v1.2.0/Compositor/IO/PSD), [Outer Glow](https://github.com/robbietilton/Compositor/pull/55), [brush smoothing](https://github.com/robbietilton/Compositor/commit/8b0215e52150453d790c31ecb87e88f50037612a), and [project validation](https://github.com/robbietilton/Compositor/blob/v1.2.0/Compositor/IO/ProjectStore.swift). The release also fixes macOS Color Dodge/Burn color-space handling and duplicate alpha conversion in Levels; Linux implements those paths separately. Rulers, guides and object selection were already included in the previous comparison.

## Added since v0.2.0

| Feature | Linux behavior |
| --- | --- |
| TIFF and WebP export | File menu commands for 8-bit RGBA TIFF and lossless WebP, preserving transparency. |
| Nikon RAW Develop | Native NEF/NRW decoding, floating-point development, Vulkan processing, white balance, tone/HSL, noise reduction/sharpening, manual lens/crop controls, local brush/gradient masks, presets, comparison views and metadata. Editable embedded sources and direct 16-bit sRGB TIFF export. See [RAW workflow](linux-raw.md). |
| Object selection refinements | Edge offset from −10 to +10 px, geometric outline smoothing, and remappable Tab switching between Wand and Object tools. Positive Edge contracts the result before selection combination. |

## Added since v0.1.0

| Feature | Linux behavior |
| --- | --- |
| Editable text | Point and paragraph text, installed fonts and styles, size, color, alignment, tracking and leading. Editable `.comp` metadata and cached pixels. A modal editor provides live preview. |
| Layer effects | Editable inside/outside stroke, drop shadow, color overlay and inner shadow, with GPU rendering, visibility, copying and undo. |
| Line shapes | Round ends, adjustable width, Shift angle snapping and Alt center drawing. Remain editable through resizing and project saving. |
| Selection feathering | Select > Feather and direct header control, 1–250 px, GPU Gaussian blur, repeated feathering, retained outline and undo. |
| Soft Light | Fourteenth blend mode on CPU and GPU. |
| Folder opacity and duplication | Pass-through opacity multiplies descendant layers. Duplication preserves nested groups, masks and clipping links. |
| Keyboard shortcuts | Searchable editor, custom bindings, conflict detection, reset and local persistence. |
| Folded distortion | Concave and crossed corners render as two triangles; collapsed triangles are rejected. |
| PSD import/export | Layered 8-bit RGB/grayscale PSD, groups, opacity, supported blends, masks and clipping. Editable rectangle, rounded rectangle and ellipse imports; editable Levels, Curves and Hue/Saturation import/export. Conversion reports precede unsupported conversions. Export is separate from saving `.comp`. |
| Object selection / Select Subject | SAM 3.1 selects individual objects from point/box prompts, including touching instances; BiRefNet selects the whole foreground. Cached image embeddings, This Layer/All Layers, antialiasing and selection combination modes. |
| Rulers, guides and grid | Pixel rulers, drag guides to create/move/delete, lock/clear, 8 px grid, selectable snapping targets, saved guides, resize/flip/crop handling and undo. |
| Open from Clipboard | Opens clipboard images in a new image-sized document without replacing existing tabs. |

PSD text and smart objects use cached raster pixels when available. Supported primitives remain editable on import; other solid vector paths can rasterize from geometry without cached pixels. Shape export rasterizes. Photoshop effects and unsupported adjustments are reported before conversion. Editable shape imports omit Photoshop strokes; rasterized vector strokes use solid, centered strokes, with unsupported alignment, dashes and blending reported. [Upstream PSD PR #39](https://github.com/robbietilton/Compositor/pull/39), merged for macOS 1.2.0, adds import only. Its editable shape imports also omit strokes; its raster stroke rendering differs from this implementation. PSB, CMYK and non-8-bit PSD files are rejected. Import and native-project persistence are tested with Photoshop CS6, CC 2019 and 22.5 files. Reopening our exports in the Photoshop application remains unverified.

## Compatibility and platform differences

- **Projects:** reads schema versions 1 through 8 and writes version 7 `.comp` directory packages, or version 8 when guides are present. Editable text, layer effects, line shapes, Soft Light and folder opacity are preserved. Unknown project fields are rejected where the schema requires it. Opening Linux-saved projects in the real macOS app remains unverified. See [project I/O](../src/project.rs) and [compatibility tests](../tests/project_compatibility.rs).
- **Background removal:** full BiRefNet Dynamic replaces Apple's proprietary Vision model. Basic/Advanced refinement, editable masks, selection handling and undo are implemented. One AppImage bundles native ONNX Runtime, the WebGPU/Vulkan plugin and both GPU/CPU models. Compatible NVIDIA and AMD Vulkan GPUs need FP16 shader support; no compatible adapter selects CPU. NVIDIA and CPU paths have been tested, physical AMD hardware has not. GPU execution failures report an error rather than silently rerunning on CPU.
- **Rendering:** large brushes, compositing and preview reduction have GPU paths with CPU fallbacks. Tests compare those paths with the Rust CPU reference; they do not establish identical macOS pixels or performance. The raster budget is 100 million pixels, and sparse canvases support up to 30,000 pixels per side.
- **Desktop:** Wayland and X11, Ctrl/Alt shortcuts, portal file dialogs and native clipboard integration. Linux window styling differs from AppKit. AppImage updates open GitHub Releases; they do not use Sparkle or replace the mounted executable.
- **Distribution:** x86_64 AppImage, glibc 2.39+, host graphics drivers and desktop portals. Releases are unsigned. No ARM64 AppImage, Flatpak, DEB or RPM is currently provided.

## Comparison with Xuan

[Xuan](https://github.com/silverling/xuan) is the Linux port linked in [upstream issue #19](https://github.com/robbietilton/Compositor/issues/19#issuecomment-5744154350). The comparison below uses its [README](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/README.md) and [user guide](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/docs/USAGE.md) at `0653436`, not a hands-on benchmark or an audit of every feature.

The README's additional status rows were checked against Xuan at the same revision: [layer data](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/src/document.rs) has no layer-effect stack; [shape kinds](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/src/paint.rs) exclude lines; [blend modes](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/src/blend.rs) exclude Soft Light. [Layer operations and tests](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/src/operations.rs) cover folder duplication and opacity; the [menus](https://github.com/silverling/xuan/blob/0653436dd3590db926a239b17cc48aa061acbb52/src/app/menus.rs) expose selection feathering and the keyboard-shortcuts dialog.

| Area | This fork | Xuan's documented behavior |
| --- | --- | --- |
| Project format | Reads and writes original `.comp` packages, with the limits above | Imports `.comp` v1–7; saves `.xuan` |
| Background removal | Bundled offline BiRefNet neural segmentation, Vulkan/CPU inference | Border-color matte for simple backgrounds |
| Healing and content-aware fill | Reuses upstream's portable C kernels | Portable texture-matching implementation with differing results |
| Imported color profiles | Little CMS conversion to sRGB, including RGB, grayscale, CMYK and Lab | Imported raster ICC profiles are not converted or preserved |
| Text and RAW | Editable text; Nikon NEF/NRW development | Editable text; Nikon NEF/NRW development |
| Image export | PNG, JPEG, TIFF and WebP; direct 16-bit TIFF from RAW Develop | PNG, JPEG, TIFF and WebP; direct 16-bit TIFF from RAW Develop |
| HEIC import | libheif decoder bundled in the AppImage | Optional external `heif-convert` |
| Distribution | One AppImage with background-removal runtime/models included | DEB, RPM and portable archive |

Both projects provide layered editing, masks, transforms, selections, retouching, adjustments, RAW development, and Wayland/X11 support. This fork also supports original project packages, color-managed imports, PSD interchange and offline neural object selection and background removal. Neither this table nor a test count establishes an overall winner.

## Verification of the current source

The ordinary suite passes 716 tests; 26 hardware or external-fixture tests are opt-in. The focused RAW hardware and Nikon fixture checks also pass. Formatting, application Clippy and the optimized Linux build pass.

The published v0.2.0 AppImage was built on Ubuntu 24.04. Checks against its extracted payload passed for both BiRefNet background removal and SAM 3.1 object selection on CPU and NVIDIA Vulkan. Packaging checks verify model hashes, native dependencies, launcher, icon and the HEIC decoder. The inference payload contains no Python runtime code or wheels. A clean Ubuntu model export reproduced every pinned output hash.

The local v0.3.0 AppImage also passes the payload checks and opens a real Nikon RAW file through its bundled launcher in an isolated X11 session. It uses the same pinned inference models and libraries as v0.2.0.

Focused checks exercise real Vulkan compositing, all four effects, Gaussian feathering, BiRefNet background removal and native prompted object selection. Effects and grayscale blur match the CPU reference within one byte. Object-model comparisons use 13 labeled instances across five DAVIS photographs, with identical point or box prompts. See [model selection and measured results](object-selection-models.md) for quality, timing, export verification and reproduction details.

Project tests cover text/effects/guides together, folder duplication and opacity, line geometry, folded distortion, and guide persistence through resizing and cropping. PSD checks cover malformed input, masks, clipping, conversion reports, editable primitives and adjustments, and native-project preservation. Three independently sourced Photoshop-created files verify import and subsequent project/PSD conversions; fetch them with `scripts/fetch-psd-fixtures.sh` and run `cargo test --locked --test psd_external -- --ignored --test-threads=4`. QuickGUI interaction tests exercise the new dialogs, clipboard opening, selection modifiers, shortcuts and guide gestures. Headless screenshots of the new text, effects, shortcuts and guide surfaces were inspected.

Real macOS and Photoshop application round trips, physical AMD hardware, and pixel-identical output across platforms remain unverified. Text editing uses a dialog; paragraph dimensions reflow through its controls, while transform handles scale the text.

RAW checks cover a real Nikon D70 file, GPU/CPU agreement, processing controls, 16-bit TIFF precision and ICC data, embedded-source project round trips, protected edits and undo. QuickGUI tests exercise numeric entry, masks, presets and preview display. Native X11 checks exercise asynchronous RAW open, development into a layer, reopening with retained settings and cancellation. Full-resolution preview tiles and the Develop layout were inspected at multiple window sizes.
