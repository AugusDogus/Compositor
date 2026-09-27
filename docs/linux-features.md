# Linux features and compatibility

The Linux development version targets feature parity with **Compositor for macOS 1.3.3**. See the [feature comparison](../README.md#features-and-parity) for supported editing tools, or [release notes](https://github.com/AugusDogus/Compositor/releases) for a specific download.

## Unreleased

- Multi-stop gradients support 2–32 color/opacity stops, numeric or dragged positioning, linear/radial painting and mask gradients.
- Dodge, Burn and Sponge brushes support pressure, selection-aware strokes and undo. Dodge/Burn offer tonal ranges; Sponge can saturate or desaturate. Large unselected strokes use Vulkan when available.
- Unsharp Mask sharpens raster layers with amount, radius and threshold controls, selection-aware preview and undo.
- High Pass isolates edges around neutral gray, with radius control, selection-aware preview and undo.
- Still GIF import/export supports up to 256 colors. Export makes alpha below 50% transparent and the rest opaque; animation editing is not supported.
- AVIF export uses bundled AOM codecs, with lossy RGB color and full 8-bit transparency.
- Edit > History browses retained undo/redo states and marks the saved state. History lasts only while the document is open.
- PSD export retains supported layer styles as editable effects. Masked, clipped, nonuniformly scaled or non-Normal-blend effects use a baked fallback.
- PSD/PSB imports convert embedded ICC profiles to sRGB for pixels and editable text/shape colors. v0.6.1 ignores these profiles.
- Compressed SVG (`.svgz`) import, with a 16 MiB limit on expanded SVG data.
- Image-menu commands rotate the whole canvas 90° in either direction, retaining editable layers and rotating masks, selections and guides. Visible Grain and Add Noise adjustments must be merged with their underlying layers first to preserve their patterns.
- DEB, RPM and Arch packaging includes the same codecs and offline models as the AppImage. These packages are not part of v0.6.1.
- A Nix flake installs the published v0.6.1 AppImage with its bundled models.

## New in v0.6.1

Changes since v0.5.0:

| Feature | Behavior |
| --- | --- |
| Project v10 and text colors | Reads v1–10 and writes v10, preserving independent colors for selected characters. |
| Dither | Atkinson, Floyd-Steinberg, Bayer, halftone, Mac patterns and ASCII, with preview and undo. |
| SVG import | Imports SVG as pixels, including text and embedded images. |
| Open Recent | Persistent recent-project list with missing files filtered out. |
| External project changes | Reloads clean projects automatically when idle; otherwise offers the external version separately. Competing saves preserve both versions. |
| Background saves | Continue editing during saves. Later edits remain unsaved; close and quit wait for the requested save. |
| Large PSD/PSB layers | Imports oversized layers by cropping them to the canvas when the full layer exceeds the memory budget. |
| Adjustment controls | Colored tracks, individual double-click resets and numeric label dragging. |
| Tablet input | Wayland and X11 pressure, tilt and eraser-tip input. Physical tablet hardware remains unverified. |
| Crash recovery | Saves snapshots every 30 seconds and restores interrupted sessions at startup. |

Recovery preserves committed document contents, not undo history or unfinished dialog edits. Changes since the last completed snapshot can be lost after a crash.

## File compatibility

- **Compositor projects:** v0.6.1 reads `.comp` versions 1–10 and writes v10. Linux v0.5.0 reads v1–9 and writes v9. Older applications may not open newly saved projects. [Mac reader/writer tests](macos-compatibility.md) preserved document data and cached pixels; full macOS application round trips remain unverified.
- **Camera RAW:** original camera data and development settings stay editable in Linux projects. Saving through the upstream Mac writer discards this Linux extension while preserving the developed pixels. Keep the original Linux package for redevelopment. Camera support depends on the bundled decoders; Foveon X3F is unsupported. See [RAW workflow and limits](linux-raw.md).
- **PSD/PSB import:** accepts 8-bit RGB and grayscale files. Supported primitives and simple point/paragraph text remain editable. Unsupported text, smart objects and some vector content use cached pixels. Missing fonts and unsupported styles or transforms are reported. CMYK and non-8-bit files are unsupported.
- **PSD export:** rasterizes text and shapes, preserves supported adjustment layers, masks and clipping, and reports conversions. Imports have been tested with Photoshop-created files; reopening exports in Photoshop remains unverified.
- **PSD adjustments:** import and export preserve Levels, Curves, Hue/Saturation, Black & White, Color Balance and Invert as editable adjustment layers.
- **PSD effects:** supported solid stroke, drop/inner shadow, color overlay and outer/inner glow styles remain editable on unmasked, Normal-blend raster layers outside clipping stacks. Unsupported imports are reported. Export bakes incompatible combinations; style opacity rounds to whole percentages, and Photoshop's stroke/blur rendering may differ. The flattened export preview retains the original rendered appearance.
- **Color and precision:** image imports, including supported PSD/PSB files, convert embedded ICC profiles to sRGB. PSD layer pixels and editable text/shape colors are converted; blending and adjustments may render differently in the sRGB working space. The compositor and ordinary exports use 8-bit color. RAW Develop can export the developed image directly as 16-bit sRGB TIFF.

## Editing and platform limits

Moving, scaling, rotating and flipping whole raster layers preserve their source pixels. Applying perspective distortion resamples pixels and rasterizes editable text and shapes. Selected-pixel transforms also resample pixels; Undo can restore the prior state while it remains in history.

Camera Raw Filter adjusts existing image pixels. [Camera RAW Develop](linux-raw.md) works from the original sensor data and preserves editable development settings. They are separate workflows.

Background Removal and Select Subject use BiRefNet; Object Selection uses SAM 3.1 with click or box prompts. Tab switches between Wand and Object Selection; Edge adjusts the detected boundary, and Anti-alias smooths its outline. The AppImage bundles the models and native inference libraries for offline use, with no Python setup. Ambiguous object boundaries may need manual correction.

NVIDIA and AMD Vulkan GPUs need FP16 support for inference. CPU inference is selected when no compatible GPU is available; GPU execution failures report an error. NVIDIA and CPU paths have been tested, but physical AMD hardware remains unverified.

Individual image surfaces are limited to 200 megapixels. Total raster storage is limited to 200–800 megapixels depending on system memory. Sparse canvases support up to 30,000 pixels per side. Text with more than 4,096 color spans uses neutral colors in the text editor with a notice; its preview and saved colors are preserved.

Bloom and background removal use different implementations from macOS. Pixel-identical rendering, font appearance and performance across operating systems are not guaranteed.

Linux supports Wayland and X11, Ctrl/Alt shortcuts, portal file dialogs and native clipboard integration. AppImage, DEB, RPM and Arch packages require x86_64, glibc 2.39+, host graphics drivers and desktop portals. Development builds produce all four formats; v0.6.1 provides the AppImage. The Nix package wraps that published AppImage. ARM64 and Flatpak builds are not provided. Releases are unsigned. Update checks open GitHub Releases; replace the AppImage or update through your package manager after closing the editor.

## Comparison sources

The README comparison was checked September 26, 2026 against [upstream `2309a85`](https://github.com/robbietilton/Compositor/tree/2309a85601824465aac5ebc1f45c4b8b9f78a5c1) (macOS 1.3.3 with its ASCII update) and [Xuan `7fd0ee1`](https://github.com/silverling/xuan/tree/7fd0ee19344c83b586346395a38c9468897c40ec) (0.2.2). Xuan coverage is based on source and documentation, not a hands-on benchmark.

Xuan imports `.comp` v1–7 and saves its own `.xuan` format. Its attached filter stacks and standalone mask layers differ from Compositor's adjustment layers. Its RAW support covers Nikon NEF/NRW and Canon CR2/CR3/CRW. Xuan also distributes Linux DEB/RPM packages and a Windows ZIP; this fork distributes a Linux AppImage.

Xuan sources: [README](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/README.md), [user guide](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/docs/USAGE.md), [blend modes](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/blend.rs), [document model](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/document.rs), and [filters](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/effects.rs).
