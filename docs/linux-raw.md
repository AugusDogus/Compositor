# Camera RAW Develop

Use **File > Open RAW** for a new document, or **Import Images** to develop a supported camera RAW file into the current project. Command-line files and dropped files use the same workflow. Multiple RAW files are queued. Decoding runs in a worker; Cancel preserves existing layers and never modifies the camera file.

## Development controls

- **Basic:** as-shot white balance, temperature/tint, lighting presets, neutral picker, auto exposure, exposure, brightness, contrast, highlights/shadows, white/black points, clarity, texture, dehaze, vibrance and saturation.
- **Tone:** five-point master/RGB curves, eight-band HSL, monochrome mixing, shadow/highlight split toning and balance.
- **Detail:** luminance/chroma noise reduction and sharpening with radius and threshold. Use 100% or full-resolution preview to inspect sensor detail.
- **Lens:** manual distortion, chromatic aberration, purple defringing, vignette, rotation, perspective and crop.
- **Masks:** linear/radial gradients and soft brush strokes with local exposure, warmth and saturation. Masks support visibility, inversion, naming and deletion. Up to 32 masks and 8,192 total brush points are retained.
- **Inspection:** edited/original, split and side-by-side views, RGB histogram, clipping indicators, zoom/pan and shooting metadata.

Presets save and load validated settings. Development has its own Undo/Redo. **Develop** commits one document edit. Double-click a RAW layer, or choose **Layer > Develop RAW**, to reopen its embedded source with the previous settings. Cancelling redevelopment leaves the committed layer unchanged.

## Precision and editing

White balance and exposure use floating-point camera data before conversion to sRGB. Highlights above the display range are retained for adjustment; sensor-saturated detail cannot be recovered.

The compositor uses an 8-bit sRGB raster. Develop's **16-bit TIFF** exports directly from the floating-point pipeline with an sRGB ICC profile, including crop and local masks. It exports the RAW image alone. File-menu TIFF/WebP export renders the whole composition at 8-bit; WebP is lossless. Neither output includes comparison or clipping overlays.

Move, rotation, scaling, convex perspective, masks, blending, groups and duplication preserve RAW editability. Direct painting, destructive filters and folded distortions require **Layer > Rasterize RAW Layer**, which is undoable. Image Size preserves RAW sources; rotated layers without perspective require proportional dimensions or rasterization when unequal scaling would introduce shear.

## Saved projects and limits

`.comp` packages embed source bytes in `raw/` and development metadata in `linux-raw.json`. The upstream manifest saves as version 10 with ordinary cached PNGs. RAW editability is a Linux extension: the upstream Mac writer preserves developed pixels but drops the embedded RAW source and settings when saving. Keep the original Linux package for redevelopment. See [cross-device tests](macos-compatibility.md).

Development builds check that saved RAW sources and settings match their cached pixels. A mismatch stops loading and leaves the files intact. Restore a matching backup, or remove `linux-raw.json` from a copy of the package to open its cached pixels without RAW editing. Older Linux RAW packages remain readable; saving upgrades their RAW metadata, which requires a current Linux build to reopen.

Camera support follows Rawler 0.7.2 and the bundled LibRaw camera decoders. Foveon X3F color development is not supported. Sources are limited to 512 MiB per project and decoded images to 200 megapixels. Processing uses Vulkan where supported, with CPU processing when hardware is unavailable or its limits are too small. Full-resolution previews display tiles without reducing their pixel detail. GPU execution failures are reported.

Lens correction is manual, noise reduction uses conventional filters, and there is no lens-profile database, sensor-saturation reconstruction or wide-gamut/HDR compositor. Shooting metadata remains in the project; TIFF output does not copy shooting EXIF.
