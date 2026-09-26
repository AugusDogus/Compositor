> [!NOTE]
> AI SLOPFORK

# Compositor for Linux

A layered image editor for Linux, built with Rust and [QuickGUI](https://github.com/egoist/quickgui). A fork of [Robbie Tilton's Compositor](https://github.com/robbietilton/Compositor).

![Compositor on Linux, with an editable background-removal mask and transform controls](docs/screenshots/workspace.png)

## Download

**[Download AppImage (x86_64)](https://github.com/AugusDogus/Compositor/releases/latest)**

```sh
chmod +x Compositor-0.5.0-x86_64.AppImage
./Compositor-0.5.0-x86_64.AppImage
```

## Requirements

| Component | Requirement |
| --- | --- |
| Platform | x86_64 Linux, glibc 2.39+ |
| Desktop | Wayland or X11; XDG portals for file dialogs |
| Graphics | Working Vulkan or OpenGL driver |
| GPU background removal and object selection | NVIDIA/AMD Vulkan GPU with FP16 support; CPU used if unavailable[^background] |

[Launch options and troubleshooting](docs/linux-releases.md#running-an-appimage)

## Features and parity

Development targets feature parity with **Compositor for macOS 1.3.3**. The table compares development with Xuan 0.2.2; the download above is the published v0.5.0 release. [Changes since v0.5.0](docs/linux-features.md#unreleased-features).

✅ Supported · ⚠️ Partial or limited · ❌ Not supported

| Feature | [Compositor<br>(macOS&nbsp;1.3.3)](https://github.com/robbietilton/Compositor) | This fork (development) | [Xuan](https://github.com/silverling/xuan) |
| --- | :---: | :---: | :---: |
| Layers, groups and masks | ✅ | ✅ | ✅ |
| Blend modes and adjustment layers | ✅ 24 modes | ✅ 24 modes | ✅ 13 modes |
| Brushes, clone, healing and content-aware fill | ✅ | ✅ | ✅ |
| Brush stroke smoothing | ✅ | ✅ | ✅ |
| Tablet pressure, tilt and eraser tip | ❌ | ⚠️ Wayland/X11[^tablet] | ✅ |
| Gradients, rectangle and ellipse shapes | ✅ | ✅ | ✅ |
| Non-destructive transforms, perspective and snapping | ✅ | ✅ | ✅ |
| Marquee, lasso and magic-wand selections | ✅ | ✅ | ✅ |
| Color adjustments and filters | ✅ | ✅ | ✅ |
| Dither, halftone and ASCII filters | ✅ | ✅ | ❌ |
| Numeric label dragging and colored adjustment tracks | ✅ | ✅ | ❌ |
| Crop, canvas and image resizing | ✅ | ✅ | ✅ |
| Image Trim | ✅ | ✅ | ❌ |
| Camera Raw Filter | ✅ | ✅ | ❌ |
| Black & White, Color Balance and Invert adjustment layers | ✅ | ✅ | ⚠️ Invert |
| Gaussian Blur, Motion Blur and Add Noise layers | ✅ | ✅ | ⚠️ Filter layers[^filters] |
| Vignette, Bloom and Tonal Contrast | ✅ | ✅ | ⚠️ Vignette[^filters] |
| Background removal | ✅ Apple Vision[^background] | ✅ BiRefNet[^background] | ⚠️ Border-color matte[^background] |
| Original `.comp` projects | ✅ Read/write | ⚠️ Read/write[^projects] | ⚠️ Import only[^projects] |
| ICC color conversion | ✅ | ✅ | ❌ |
| HEIC import | ✅ | ✅ Bundled | ✅ Bundled |
| SVG import | ✅ Rasterized | ✅ Rasterized | ❌ |
| PNG/JPEG export | ✅ | ✅ | ✅ |
| TIFF/WebP export | ❌ | ✅ | ✅ |
| Editable text | ✅ | ✅ | ✅ |
| Per-character text colors | ✅ | ✅ | ❌ |
| Camera RAW development | ✅ | ✅[^raw] | ⚠️ Nikon/Canon[^raw] |
| Stroke, drop shadow, color overlay and inner shadow | ✅ | ✅ | ❌ |
| Outer Glow and Inner Glow | ✅ | ✅ | ❌ |
| Line shapes | ✅ | ✅ | ❌ |
| Selection feathering | ✅ | ✅ | ✅ |
| Soft Light blend mode | ✅ | ✅ | ❌ |
| Folder opacity and duplication | ✅ | ✅ | ✅ |
| Keyboard-shortcuts editor | ✅ | ✅ | ⚠️ Reference window |
| PSD/PSB import | ⚠️ Layers, shapes, text, adjustments[^upstream-psd] | ⚠️ Layers, shapes, text, adjustments[^psd] | ❌ |
| PSD export | ❌ | ⚠️ Layers and adjustments[^psd] | ❌ |
| Object selection | ✅ Click | ✅ Click/box[^objects] | ❌ |
| Object-selection edge adjustment and smoothing | ✅ | ✅ | ❌ |
| Select Subject | ✅ | ✅ BiRefNet[^objects] | ❌ |
| Rulers, guides and layout grid | ✅ | ✅ | ❌ |
| Open from Clipboard | ❌ | ✅ | ❌ |
| Open Recent and external project reload | ✅ | ✅ | ❌ |
| Continue editing during project saves | ✅ | ✅ | ❌ |
| Autosave and crash recovery | ❌ | ✅[^recovery] | ❌ |

[^background]: This fork bundles BiRefNet, SAM 3.1 and native inference libraries for offline use, with no Python setup. NVIDIA and CPU inference are tested; AMD hardware is unverified. Background-removal results differ from Apple's Vision model. Xuan's border-color matte is intended for simple backgrounds.
[^projects]: Development reads `.comp` v1–10 and writes v10, preserving per-character text colors and all six effects. [15 round trips through upstream Swift on a Mac passed](docs/macos-compatibility.md); full-app testing remains unverified. Mac saves discard this fork's embedded RAW source/settings. Xuan imports `.comp` and saves `.xuan`.
[^upstream-psd]: macOS 1.3.3 imports 8-bit RGB PSD/PSB with supported shapes, simple text and adjustments. Unsupported text and smart objects become pixels; Photoshop effects are discarded and unsupported conversions are reported. No PSD export.
[^psd]: 8-bit RGB/grayscale PSD/PSB import; PSD export. Imports editable primitives and supported point/paragraph text; imports/exports Levels, Curves, Hue/Saturation, Black & White, Color Balance and Invert. Shape/text exports are rasterized. Unsupported text and smart objects use cached pixels; unsupported conversions are reported. Tested with Photoshop-created files; reopening exports in Photoshop remains unverified.
[^objects]: Object Selection uses SAM 3.1; Select Subject uses BiRefNet. Click an object or draw a box around it. Tab switches Wand/Object; Edge adjusts the detected boundary, and Anti-alias smooths its outline. Ambiguous boundaries may need selection corrections.
[^raw]: Camera coverage depends on the bundled decoders; Foveon X3F is unsupported. Xuan supports Nikon NEF/NRW and Canon CR2/CR3/CRW. Editable development, local masks and direct 16-bit sRGB TIFF output; the compositor and ordinary TIFF/WebP exports remain 8-bit. This fork embeds the original source and settings in a Linux extension inside `.comp` packages.
[^filters]: Xuan provides editable Gaussian Blur, Motion Blur and Noise filter layers, plus vignette through Lens Correction. Its attached filter stacks differ from macOS adjustment layers.
[^tablet]: Native pen input and brush dynamics are implemented for Wayland and X11. Physical tablet hardware is not yet verified.
[^recovery]: Recovery snapshots are saved every 30 seconds while editing. They preserve committed document changes, not undo history or unfinished dialog edits. Changes since the last completed snapshot can be lost after a crash.

[Comparison sources and detailed feature status](docs/linux-features.md)

## Development

[Build from source](docs/linux-building.md) · [AppImage builds and releases](docs/linux-releases.md) · [Contributor guide](docs/linux-port.md)

## License

[MIT](LICENSE), preserving the original Compositor copyright. QuickGUI and bundled dependencies retain their own license notices. [Screenshot provenance](docs/screenshots/README.md).
