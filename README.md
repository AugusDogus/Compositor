> [!NOTE]
> AI SLOPFORK

# Compositor for Linux

A layered image editor for Linux, built with Rust and [QuickGUI](https://github.com/egoist/quickgui). A fork of [Robbie Tilton's Compositor](https://github.com/robbietilton/Compositor).

![Compositor on Wayland, with an editable background-removal mask and transform controls](docs/screenshots/workspace.png)

## Download

**[Download AppImage (x86_64)](https://github.com/AugusDogus/Compositor/releases/latest)**

```sh
chmod +x Compositor-0.3.0-x86_64.AppImage
./Compositor-0.3.0-x86_64.AppImage
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

Targets feature parity with **Compositor for macOS 1.1.8**. Linux feature status is for **v0.3.0**.

✅ Supported · ⚠️ Partial or limited · ❌ Not supported

| Feature | [Compositor<br>(macOS&nbsp;1.1.8)](https://github.com/robbietilton/Compositor) | This fork | [Xuan](https://github.com/silverling/xuan) |
| --- | :---: | :---: | :---: |
| Layers, groups and masks | ✅ | ✅ | ✅ |
| Blend modes and adjustment layers | ✅ 14 modes | ✅ 14 modes | ✅ 13 modes |
| Brushes, clone, healing and content-aware fill | ✅ | ✅ | ✅ |
| Gradients, rectangle and ellipse shapes | ✅ | ✅ | ✅ |
| Non-destructive transforms, perspective and snapping | ✅ | ✅ | ✅ |
| Marquee, lasso and magic-wand selections | ✅ | ✅ | ✅ |
| Color adjustments and filters | ✅ | ✅ | ✅ |
| Crop, canvas and image resizing | ✅ | ✅ | ✅ |
| Background removal | ✅ Apple Vision[^background] | ✅ BiRefNet[^background] | ⚠️ Border-color matte[^background] |
| Original `.comp` projects | ✅ Read/write | ⚠️ Read/write[^projects] | ⚠️ Import only[^projects] |
| ICC color conversion | ✅ | ✅ | ❌ |
| HEIC import | ✅ | ✅ Bundled | ⚠️ External helper[^heic] |
| PNG/JPEG export | ✅ | ✅ | ✅ |
| TIFF/WebP export | ❌ | ✅ | ✅ |
| Editable text | ✅ | ✅ | ✅ |
| Nikon RAW development | ❌ | ✅[^raw] | ✅[^raw] |
| Layer effects | ✅ | ✅ | ❌ |
| Line shapes | ✅ | ✅ | ❌ |
| Selection feathering | ✅ | ✅ | ✅ |
| Soft Light blend mode | ✅ | ✅ | ❌ |
| Folder opacity and duplication | ✅ | ✅ | ✅ |
| Keyboard-shortcuts editor | ✅ | ✅ | ⚠️ Reference window |
| PSD import/export | ❌ | ⚠️ Layers, shapes, adjustments[^psd] | ❌ |
| Object selection | ✅ Click | ✅ Click/box[^objects] | ❌ |
| Object-selection edge adjustment and smoothing | ✅ | ✅ | ❌ |
| Select Subject | ✅ | ✅ BiRefNet[^objects] | ❌ |
| Rulers, guides and layout grid | ✅ | ✅ | ❌ |
| Open from Clipboard | ❌ | ✅ | ❌ |

[^background]: This fork bundles BiRefNet, SAM 3.1 and native inference libraries for offline use, with no Python setup. NVIDIA and CPU inference are tested; AMD hardware is unverified. Background-removal results differ from Apple's Vision model. Xuan's border-color matte is intended for simple backgrounds.
[^projects]: This fork reads `.comp` v1–8 and writes v7 (v8 with guides), preserving editable text and effects. Real macOS round trips remain unverified. Xuan imports `.comp` and saves `.xuan`.
[^psd]: 8-bit RGB/grayscale PSD. Imports editable rectangles, rounded rectangles and ellipses; imports/exports Levels, Curves and Hue/Saturation. Shape exports are rasterized. Text/smart objects use cached pixels; unsupported conversions are reported. Tested with Photoshop-created files; reopening exports in Photoshop remains unverified.
[^objects]: Object Selection uses SAM 3.1; Select Subject uses BiRefNet. Click an object or draw a box around it. Tab switches Wand/Object; Edge adjusts the detected boundary, and Anti-alias smooths its outline. Ambiguous boundaries may need selection corrections.
[^raw]: NEF/NRW support follows Rawler's RGB Bayer camera support. Editable development, local masks and direct 16-bit sRGB TIFF output; the compositor and ordinary TIFF/WebP exports remain 8-bit. This fork embeds the original source and settings in a Linux extension inside `.comp` packages.
[^heic]: Xuan requires the optional `heif-convert` executable.

[Comparison sources and detailed feature status](docs/linux-features.md)

## Development

[Build from source](docs/linux-building.md) · [AppImage builds and releases](docs/linux-releases.md) · [Implementation notes](docs/linux-port.md)

## License

[MIT](LICENSE), preserving the original Compositor copyright. QuickGUI and bundled dependencies retain their own license notices. [Screenshot provenance](docs/screenshots/README.md).
