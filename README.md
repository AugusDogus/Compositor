> [!NOTE]
> AI SLOPFORK

# Compositor for Linux

A layered image editor for Linux, built with Rust and [QuickGUI](https://github.com/egoist/quickgui). A fork of [Robbie Tilton's Compositor](https://github.com/robbietilton/Compositor).

![Compositor on Wayland, with an editable background-removal mask and transform controls](docs/screenshots/workspace.png)

## Download

**[Download AppImage (x86_64)](https://github.com/AugusDogus/Compositor/releases/latest)**

```sh
chmod +x Compositor-0.1.0-x86_64.AppImage
./Compositor-0.1.0-x86_64.AppImage
```

## Requirements

| Component | Requirement |
| --- | --- |
| Platform | x86_64 Linux, glibc 2.39+ |
| Desktop | Wayland or X11; XDG portals for file dialogs |
| Graphics | Working Vulkan or OpenGL driver |
| GPU background removal | NVIDIA/AMD Vulkan GPU with FP16 support; CPU used if unavailable[^background] |

[Launch options and troubleshooting](docs/linux-releases.md#running-an-appimage)

## Features and parity

Targets feature parity with **Compositor for macOS 1.0.4**.

✅ Supported · ⚠️ Partial or limited · ❌ Not supported

| Feature | This fork | [Xuan](https://github.com/silverling/xuan) |
| --- | :---: | :---: |
| Layers, groups and masks | ✅ | ✅ |
| 13 blend modes and adjustment layers | ✅ | ✅ |
| Brushes, clone, healing and content-aware fill | ✅ | ✅ |
| Gradients, rectangle and ellipse shapes | ✅ | ✅ |
| Non-destructive transforms, perspective and snapping | ✅ | ✅ |
| Marquee, lasso and magic-wand selections | ✅ | ✅ |
| Color adjustments and filters | ✅ | ✅ |
| Crop, canvas and image resizing | ✅ | ✅ |
| Background removal | ✅ BiRefNet[^background] | ⚠️ Border-color matte[^background] |
| Original `.comp` projects | ⚠️ Read/write[^projects] | ⚠️ Import only[^projects] |
| ICC color conversion | ✅ | ❌ |
| HEIC import | ✅ Bundled | ⚠️ External helper[^heic] |
| PNG/JPEG export | ✅ | ✅ |
| TIFF/WebP export | ❌ | ✅ |
| Editable text | ❌ | ✅ |
| Nikon RAW development | ❌ | ✅ |
| Layer effects | ❌ | ❌ |
| Line shapes | ❌ | ❌ |
| Selection feathering | ❌ | ✅ |
| Soft Light blend mode | ❌ | ❌ |
| Folder opacity and duplication | ❌ | ✅ |
| Keyboard-shortcuts window | ❌ | ✅ |

[^background]: This fork bundles BiRefNet and native inference libraries for offline use, with no Python setup. NVIDIA and CPU inference are tested; AMD hardware is unverified. Results differ from Apple's Vision model. Xuan's border-color matte is intended for simple backgrounds.
[^projects]: This fork reads `.comp` v1–7 and writes v7; newer text/effect metadata and other unsupported features are rejected. Real macOS round trips remain unverified. Xuan imports `.comp` and saves `.xuan`.
[^heic]: Xuan requires the optional `heif-convert` executable.

[Comparison sources and detailed feature status](docs/linux-features.md)

## Development

[Build from source](docs/linux-building.md) · [AppImage builds and releases](docs/linux-releases.md) · [Implementation notes](docs/linux-port.md)

## License

[MIT](LICENSE), preserving the original Compositor copyright. QuickGUI and bundled dependencies retain their own license notices. [Screenshot provenance](docs/screenshots/README.md).
