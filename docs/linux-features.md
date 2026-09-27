# Linux features and compatibility

The Linux development version targets feature parity with **Compositor for macOS 1.3.3**. See the [feature comparison](../README.md#features-and-parity) for supported editing tools, or [release notes](https://github.com/AugusDogus/Compositor/releases) for a specific download.

## Unreleased

- Blend If fades a pixel, text, shape or RAW layer using independent black/white split handles for its own Gray tones and the underlying layers. Masks, clipping stacks, preview and undo remain editable. Partial merges must include the backdrop and clipping context used by Blend If. Gray is measured from nearest-byte RGB; transparent backdrops do not exclude a layer. Linux projects retain disabled settings; PSD retains active Gray ranges. Folder, adjustment-layer and channel-specific PSD ranges are unsupported, and Photoshop rendering can differ.

- Shadows/Highlights lifts dark areas and reduces bright areas using a configurable neighborhood radius. Direct pixel edits and editable adjustment layers support preview, masks and undo. Vulkan accelerates the neighborhood blur. Linux projects retain the settings; macOS and PSD receive a rendered composite.

- Vibrance adjusts color intensity with protection for skin tones, alongside a separate Saturation control. Use it as a pixel edit or an editable adjustment layer, with preview and undo. Linux projects retain the settings; PSD keeps whole-number slider values editable and renders fractional values.

- Bevel/Emboss adds editable Inner Bevel, Outer Bevel and Emboss lighting, with size, depth, direction, altitude and highlight/shadow opacity. Preview, copy and undo preserve source pixels. Linux projects retain the settings; macOS and PSD receive rendered layers.

- Gradient Overlay recolors a layer with linear or radial gradients. Edit color and opacity stops independently, with angle, reverse, opacity, preview and undo. Linux projects keep the settings; macOS and PSD receive rendered layers.

- Pattern Overlay tiles imported Photoshop PAT patterns over a layer, with scale, opacity, preview and undo. Import supports 8-bit RGB and grayscale packs. Linux projects embed selected patterns; macOS and PSD receive rendered layers. Unused patterns remain available while the effects dialog is open.

- Posterize reduces each color channel to 2–256 levels; 256 leaves colors unchanged. Direct pixel edits and editable adjustment layers support preview, masks and undo. Linux projects and PSD exports retain the settings.

- Threshold converts colors to black or white at a chosen luminance level (0–255). Use it as a pixel edit or an editable adjustment layer, with preview, masks and undo. Linux projects and PSD exports preserve the adjustment; macOS receives a rendered compatibility copy.

- Selective Color adjusts cyan, magenta, yellow and black within nine color ranges, using Relative or Absolute amounts. It supports direct pixel edits and editable adjustment layers with masks, preview and undo. Vulkan accelerates both paths. Linux projects retain all settings; PSD keeps integer amounts editable and uses a rendered copy for fractional amounts.

- Artboards group layers inside editable frames with transparent or colored backgrounds. Create them empty or from selected layers, move them by their labels, and resize their frames without scaling the contents. Export Artboards writes one PNG per visible board, excluding other boards and adjustments outside the board. Linux projects preserve editable artboards; macOS and PSD exports use a rendered compatibility copy.

- File > Export Layers writes a PNG for each visible pixel, text or shape layer, with its clipping stack, masks, effects and opacity. Files use each layer’s transformed bounds and stay within the canvas. Unclipped adjustment layers are excluded. Each batch gets a new folder.

- Path shapes retain editable Bézier anchors, handles, fill and stroke. Use Create Shape in the Pen toolbar, then double-click the layer to edit its points. Shape Style accepts `none`, `#RRGGBB` or `#RRGGBBAA`; stroke width uses source pixels and scales with the layer. Linux projects keep the geometry; macOS and PSD receive rendered layers. Rasterize Path Shape enables destructive pixel editing.
- Pen (P) creates saved Bézier paths with editable anchors and handles. Convert paths to selections, fill them, or stroke them with the current brush and mask target. Paths support undo, renaming and canvas transforms. Linux `.comp` files preserve them; macOS and PSD exports omit the working paths.

- View > Theme offers Dark, Light and live Omarchy colors. Omarchy reads `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (default `~/.local/state/omarchy/current/theme/colors.toml`) every two seconds. Invalid palettes keep the last valid appearance; image colors stay unchanged.
- Luminosity Sharpen sharpens brightness edges; Reduce Noise limits sharpening of small variations. It edits pixels with selection-aware preview and undo. Vulkan processes supported image sizes, with CPU fallback for unavailable GPUs or larger images.
- File > Export Sizes writes up to 16 PNG or JPEG sizes into a new folder, or creates editable artboards in the project. Choose social, video and print presets or custom dimensions. Fit adds padding; Fill crops centrally. Editable copies retain layers, masks, text and effects, with one undo step for the batch. In projects with artboards, select one board to copy. Merge Grain and Add Noise layers before creating editable variants. Image-file exports leave the project unchanged; PNG keeps transparency and JPEG uses white.
- Channel Mixer adjusts each output channel with signed red, green and blue contributions plus a constant, or produces monochrome output. Use it as a pixel edit or an editable adjustment layer with masks, opacity, preview and undo. Linux projects preserve its settings; macOS opens a rendered compatibility layer. PSD retains integer coefficients; fractional mixes require a rendered copy.
- Photo Filter adds warming, cooling, sepia and custom-color filtration, with density and Preserve Luminosity controls. Use it as a pixel edit or an editable adjustment layer with masks, opacity, preview and undo. Vulkan accelerates processing when available. Linux `.comp` projects retain the editable settings; macOS opens a rendered compatibility layer. PSD export flattens projects containing these layers, with a conversion notice.
- ABR import lets you choose sampled tip shapes from Photoshop brush packs, with a shape preview and original spacing. Photoshop dynamics, dual brushes, tip transforms and embedded texture patterns are not imported.
- GIH image pipes choose among brush cells using incremental, random, angular, pressure or tilt rules. Velocity rules are unsupported. Like GBR tips, embedded colors supply alpha coverage; painting uses the foreground color.
- GBR v2 brush tips support imported shapes, spacing, pressure, tilt, selections and undo. Paint and Erase use the foreground color or mask coverage; embedded RGB colors are not used. ABR, GBR and GIH brushes stay loaded for the current session and can be unloaded from Brush Tips.
- Brightness/Contrast provides direct pixel edits and adjustment layers with preview and undo. It saves as native Levels; Mac saves retain those Levels but remove the Linux slider settings.
- Radial Blur provides Spin and Zoom modes with center controls, selection-aware preview and undo. Vulkan accelerates supported image sizes, with a CPU path when unavailable.
- Edit > Fade adjusts the opacity of the last raster edit with preview and undo. It requires unchanged pixel dimensions and layer placement; geometry, mask and multi-layer edits cannot be faded.
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
- **PSD adjustments:** import and export preserve Levels, Curves, Hue/Saturation, Black & White, Color Balance, Invert, integer-coefficient Channel Mixer and integer-amount Selective Color as editable adjustment layers. Monochrome Channel Mixer exports its active gray mix; inactive RGB mixes remain in the `.comp` project.
- **PSD effects:** supported solid stroke, drop/inner shadow, color overlay and outer/inner glow styles remain editable on unmasked, Normal-blend raster layers outside clipping stacks. Unsupported imports are reported. Export bakes incompatible combinations; style opacity rounds to whole percentages, and Photoshop's stroke/blur rendering may differ. The flattened export preview retains the original rendered appearance.
- **Color and precision:** image imports, including supported PSD/PSB files, convert embedded ICC profiles to sRGB. PSD layer pixels and editable text/shape colors are converted; blending and adjustments may render differently in the sRGB working space. The compositor and ordinary exports use 8-bit color. RAW Develop can export the developed image directly as 16-bit sRGB TIFF.

## Editing and platform limits

Moving, scaling, rotating and flipping whole raster layers preserve their source pixels. Applying perspective distortion resamples pixels and rasterizes editable text and shapes. Selected-pixel transforms also resample pixels; Undo can restore the prior state while it remains in history.

Camera Raw Filter adjusts existing image pixels. [Camera RAW Develop](linux-raw.md) works from the original sensor data and preserves editable development settings. They are separate workflows.

Background Removal and Select Subject use BiRefNet; Object Selection uses SAM 3.1 with click or box prompts. Tab switches between Wand and Object Selection; Edge adjusts the detected boundary, and Anti-alias smooths its outline. The AppImage bundles the models and native inference libraries for offline use, with no Python setup. Ambiguous object boundaries may need manual correction.

NVIDIA and AMD Vulkan GPUs need FP16 support for inference. CPU inference is selected when no compatible GPU is available; GPU execution failures report an error. NVIDIA and CPU paths have been tested, but physical AMD hardware remains unverified.

Individual image surfaces are limited to 200 megapixels. Total raster storage is limited to 200–800 megapixels depending on system memory. Sparse canvases support up to 30,000 pixels per side. Text with more than 4,096 color spans uses neutral colors in the text editor with a notice; its preview and saved colors are preserved.

Bloom and background removal use different implementations from macOS. Pixel-identical rendering, font appearance and performance across operating systems are not guaranteed.

Linux supports Wayland and X11, Ctrl/Alt shortcuts, portal file dialogs and native clipboard integration. AppImage, DEB, RPM and Arch packages require x86_64, glibc 2.39+, host graphics drivers and desktop portals. Development builds produce all four formats; v0.6.1 provides the AppImage. The Nix package wraps that published AppImage. ARM64 builds for all four package formats are configured for native CI but have not yet been built or hardware-tested. Flatpak builds are not provided. Releases are unsigned. Update checks open GitHub Releases; replace the AppImage or update through your package manager after closing the editor.

## Comparison sources

The README comparison was checked September 26, 2026 against [upstream `2309a85`](https://github.com/robbietilton/Compositor/tree/2309a85601824465aac5ebc1f45c4b8b9f78a5c1) (macOS 1.3.3 with its ASCII update) and [Xuan `7fd0ee1`](https://github.com/silverling/xuan/tree/7fd0ee19344c83b586346395a38c9468897c40ec) (0.2.2). Xuan coverage is based on source and documentation, not a hands-on benchmark.

Xuan imports `.comp` v1–7 and saves its own `.xuan` format. Its attached filter stacks and standalone mask layers differ from Compositor's adjustment layers. Its RAW support covers Nikon NEF/NRW and Canon CR2/CR3/CRW. Xuan also distributes Linux DEB/RPM packages and a Windows ZIP; this fork distributes a Linux AppImage.

Xuan sources: [README](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/README.md), [user guide](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/docs/USAGE.md), [blend modes](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/blend.rs), [document model](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/document.rs), and [filters](https://github.com/silverling/xuan/blob/7fd0ee19344c83b586346395a38c9468897c40ec/src/effects.rs).
