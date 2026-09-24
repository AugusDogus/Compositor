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

Targets feature parity with **Compositor for macOS 1.2.9**. Xuan is compared at `7fd0ee1` (0.2.2).

✅ Supported · ⚠️ Partial or limited · ❌ Not supported

| Feature | [Compositor<br>(macOS&nbsp;1.2.9)](https://github.com/robbietilton/Compositor) | This fork (v0.5.0) | [Xuan](https://github.com/silverling/xuan) |
| --- | :---: | :---: | :---: |
| Layers, groups and masks | ✅ | ✅ | ✅ |
| Blend modes and adjustment layers | ✅ 24 modes | ✅ 24 modes | ✅ 13 modes |
| Brushes, clone, healing and content-aware fill | ✅ | ✅ | ✅ |
| Brush stroke smoothing | ✅ | ✅ | ✅ |
| Gradients, rectangle and ellipse shapes | ✅ | ✅ | ✅ |
| Non-destructive transforms, perspective and snapping | ✅ | ✅ | ✅ |
| Marquee, lasso and magic-wand selections | ✅ | ✅ | ✅ |
| Color adjustments and filters | ✅ | ✅ | ✅ |
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
| PNG/JPEG export | ✅ | ✅ | ✅ |
| TIFF/WebP export | ❌ | ✅ | ✅ |
| Editable text | ✅ | ✅ | ✅ |
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

[^background]: This fork bundles BiRefNet, SAM 3.1 and native inference libraries for offline use, with no Python setup. NVIDIA and CPU inference are tested; AMD hardware is unverified. Background-removal results differ from Apple's Vision model. Xuan's border-color matte is intended for simple backgrounds.
[^projects]: This fork reads `.comp` v1–9 and writes v9, preserving editable text and all six effects. Real macOS round trips remain unverified. Xuan imports `.comp` and saves `.xuan`.
[^upstream-psd]: macOS 1.2.9 imports 8-bit RGB PSD/PSB with supported shapes, simple text and adjustments. Unsupported text and smart objects become pixels; Photoshop effects are discarded and unsupported conversions are reported. No PSD export.
[^psd]: 8-bit RGB/grayscale PSD/PSB import; PSD export. Imports editable primitives and supported point/paragraph text; imports/exports Levels, Curves, Hue/Saturation, Black & White, Color Balance and Invert. Shape/text exports are rasterized. Unsupported text and smart objects use cached pixels; unsupported conversions are reported. Tested with Photoshop-created files; reopening exports in Photoshop remains unverified.
[^objects]: Object Selection uses SAM 3.1; Select Subject uses BiRefNet. Click an object or draw a box around it. Tab switches Wand/Object; Edge adjusts the detected boundary, and Anti-alias smooths its outline. Ambiguous boundaries may need selection corrections.
[^raw]: Camera coverage depends on the bundled decoders; Foveon X3F is unsupported. Xuan supports Nikon NEF/NRW and Canon CR2/CR3/CRW. Editable development, local masks and direct 16-bit sRGB TIFF output; the compositor and ordinary TIFF/WebP exports remain 8-bit. This fork embeds the original source and settings in a Linux extension inside `.comp` packages.
[^filters]: Xuan provides editable Gaussian Blur, Motion Blur and Noise filter layers, plus vignette through Lens Correction. Its attached filter stacks differ from macOS adjustment layers.

[Comparison sources and detailed feature status](docs/linux-features.md)

## Development

[Build from source](docs/linux-building.md) · [AppImage builds and releases](docs/linux-releases.md) · [Implementation notes](docs/linux-port.md)

## License

[MIT](LICENSE), preserving the original Compositor copyright. QuickGUI and bundled dependencies retain their own license notices. [Screenshot provenance](docs/screenshots/README.md).
