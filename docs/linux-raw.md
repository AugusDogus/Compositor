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

Rawler decodes sensor data, normalizes black/white levels and demosaics RGB Bayer pixels. LibRaw supplies linear 16-bit camera RGB for X-Trans and other supported cameras when Rawler cannot decode or demosaic them. White balance and exposure operate on floating-point camera values before sRGB conversion. Encoded highlights above the display range are retained; sensor-saturated detail cannot be recovered.

The compositor uses an 8-bit sRGB raster. Develop's **16-bit TIFF** exports directly from the floating-point pipeline with an sRGB ICC profile, including crop and local masks. It exports the RAW image alone. File-menu TIFF/WebP export renders the whole composition at 8-bit; WebP is lossless. Neither output includes comparison or clipping overlays.

Move, rotation, scaling, masks, blending, groups and duplication preserve RAW editability. Direct painting and destructive filters require **Layer > Rasterize RAW Layer**, which is undoable. Perspective distortion also requires rasterization. Image Size preserves RAW sources when its scaling is representable without shear; a rotated layer with unequal horizontal/vertical scaling requires proportional dimensions or rasterization.

## Saved projects and limits

`.comp` packages embed source bytes in `raw/` and development metadata in `linux-raw.json`. The upstream manifest saves as version 9 with ordinary cached PNGs. RAW editability is a Linux extension; preserving it through a save in the macOS application is not guaranteed. Keep the Linux package when exchanging rasterized output.

Camera support follows Rawler 0.7.2 and the bundled LibRaw camera decoders. Foveon X3F color development is not supported. Sources are limited to 512 MiB per project and decoded images to 200 megapixels. Processing uses Vulkan where supported, with CPU processing when hardware is unavailable or its limits are too small. Full-resolution previews display tiles without reducing their pixel detail. GPU execution failures are reported.

Lens correction is manual, noise reduction uses conventional filters, and there is no lens-profile database, sensor-saturation reconstruction or wide-gamut/HDR compositor. Shooting metadata remains in the project; TIFF output does not copy shooting EXIF.

## Verification

`scripts/fetch-raw-fixture.sh` downloads a checksum-pinned CC0 Nikon D70 file. Its output gives the command for the opt-in camera test. Synthetic tests cover highlight recovery, GPU/CPU agreement, all processing passes, source-preserving transforms and 16-bit precision. Project tests cover settings/source persistence, shared-source duplication, unsafe assets, undo and explicit rasterization.

`scripts/fetch-raw-fixtures-extra.sh` downloads a checksum-pinned CC0 Fujifilm X-Pro1 X-Trans file. Its opt-in test verifies native decoding, more than 8-bit source precision, color output and exposure redevelopment.
