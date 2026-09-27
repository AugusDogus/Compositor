# brushkit

Read Photoshop and Procreate brush files in Rust and draw their tip shapes.

Give brushkit the bytes of an `.abr`, `.brush` or `.brushset` file and you get
back every brush's name and a grayscale picture of its tip. That is enough to
show a brush pack as a grid of thumbnails or to list what a pack contains.
brushkit only reads. It never writes or converts a brush file.

## Install

```toml
[dependencies]
brushkit = "0.4"
```

`brushkit` re-exports `brushkit-abr` as `brushkit::abr` and
`brushkit-preview` as `brushkit::preview`. If you need only one of them,
depend on that crate instead. The API reference is on
[docs.rs](https://docs.rs/brushkit).

## Example

This program lists the brushes in a Photoshop pack and the size of each
preview:

```rust
use brushkit::abr::parse_abr_deferred_without_patterns;
use brushkit::preview::{preview_abr, PreviewOptions, TipPreview};

fn main() {
    let bytes = std::fs::read("pack.abr").unwrap();

    let pack = parse_abr_deferred_without_patterns(&bytes).unwrap().pack;
    println!(
        "{:?}: {} sampled brushes, {} computed presets",
        pack.version,
        pack.brushes.len(),
        pack.computed_presets.len()
    );

    let set = preview_abr(&bytes, PreviewOptions { max_cell: 128 }).unwrap();
    for entry in &set.entries {
        match &entry.tip {
            TipPreview::Available(bitmap) => {
                println!("{}: {}x{}", entry.name, bitmap.width, bitmap.height)
            }
            TipPreview::Unavailable(reason) => println!("{}: {reason:?}", entry.name),
        }
    }
}
```

To run it on your own file, put the file at `pack.abr` in the repository root
and run `cargo run -p brushkit --example readme`.

## Supported files

- Photoshop `.abr`, versions 1, 2, 6, 7, 9 and 10. You get brush names,
  sampled tips, computed tips, the full brush descriptor and embedded
  patterns. A sampled tip is a bitmap stored in the file (8 or 16 bit, raw,
  RLE or zlib). A computed tip has no bitmap. The file describes it with a
  diameter and, optionally, hardness, angle and roundness. brushkit draws the
  tip when the diameter is at least 1 pixel and uses defaults for any of the
  other three that are missing. Version 1 files store no brush names.
- Procreate `.brush`. You get the brush name from `Brush.archive` and its tip
  from `Shape.png`.
- Procreate `.brushset`. You get the set name, the brush order from
  `brushset.plist`, and the name and tip of each brush. A set without
  `brushset.plist` is read in zip order and has no name.

## Previews

`preview_abr`, `preview_brush` and `preview_brushset` return one entry per
brush, in file order. Every brush is in the list, including one whose tip
cannot be drawn. That entry carries the reason instead of a bitmap, so a grid
of thumbnails has no gaps. The reasons are `NoShapePng`, `UnsupportedTipKind`,
`Corrupt`, `TooLarge` and `OverBudget`.

`max_cell` sets the largest side of a preview in pixels, and it must be at
least 1. A larger tip is scaled down, and a smaller tip keeps its size. Each
entry also has `source_dimensions`, the size of the original tip when the file
stores one.

To draw only the first few thumbnails, use `preview_abr_first_available`,
`preview_brush_first_available` or `preview_brushset_first_available`. They
return the first `n` drawable tips and stop there. Each entry keeps its index
from the full list, so the numbers can skip.

`generate_preview_png` turns one tip into a PNG of at most 200 pixels per
side, with black ink and the tip's gray value as alpha.
`generate_contact_sheet_png` draws many tips on one labeled sheet.

## Reading an .abr pack

`brushkit::abr` has four parse functions. They differ in how much they decode
before they return:

| Function | Tips | Patterns |
|---|---|---|
| `parse_abr` | Decoded up front | Read |
| `parse_abr_deferred` | Decoded on demand. v1 and v2 tips and dual-brush tips are decoded up front. | Read |
| `parse_abr_deferred_without_patterns` | Same as `parse_abr_deferred` | Skipped |
| `parse_abr_all_deferred_without_patterns` | All decoded on demand | Skipped |

A deferred parse returns a `DeferredPack`, and `DeferredPack::decode_tip(i)`
decodes one tip when you need it. The pack borrows the input bytes, so keep
them alive while you use it. A pack's pattern block can be hundreds of
megabytes, so skip patterns when you only need tips.

## Features

- `text` (on by default) enables contact sheets. It pulls in `ab_glyph` and
  an embedded copy of the Inter Regular font for the labels.
- `serde` adds `Serialize` to the raw descriptor dump in
  `brushkit_abr::dump`.

Every crate builds for `wasm32-unknown-unknown`, because the readers take a
`&[u8]` and touch neither the filesystem nor threads.

## Untrusted files

brushkit expects files from untrusted sources. It checks every size, count
and dimension it reads against a limit before it allocates memory. A malformed
file gives an error or an unavailable entry, not a panic. One preview call
returns at most `MAX_PREVIEW_BYTES` (256 MiB) of bitmaps, and the tips after
that point come back as `OverBudget`.

Four fuzz targets under `fuzz/` cover both readers. CI replays their seed
files and fuzzes each target for 30 seconds on every pull request and every
push to `main`.

## Development

```sh
cargo build
cargo test
cargo +nightly fuzz run abr_parse   # needs cargo-fuzz
```

[fuzz/README.md](fuzz/README.md) lists the other fuzz targets and explains how
to regenerate the seeds.

Real brush packs are copyrighted, so the repository has none. Tests that check
real files read the directory in `BRUSHKIT_CORPUS_DIR` and are skipped when it
is not set. Synthetic files in the unit tests and the fuzz seeds cover the
parsing itself. `brushkit-fixture` builds
the synthetic descriptor bytes for the `brushkit-abr` tests.

The usage example above is also `crates/brushkit/examples/readme.rs`, and a
test fails if the two differ.

## Versions

brushkit is at 0.x, so a minor release can change the API. Pin the minor
version, as in the install snippet above. Each release has a `vX.Y.Z` tag.

## License

MIT, see `LICENSE`. The Inter font used by the `text` feature is under the SIL
Open Font License 1.1, see `crates/preview/assets/fonts/OFL.txt`.
