# Compositor project format

A `.comp` project is a directory package containing `manifest.json` and embedded PNG assets under `images/`. Linux reads versions **1–10** and writes **version 10**. Transfer the entire directory when moving a project between machines.

The schema is implemented in [project.rs](../src/project.rs). Tested macOS interoperability and its limits are described in [cross-platform compatibility](macos-compatibility.md).

## Stored data

The manifest identifies `com.compositor.project` and the sRGB working space. It stores document and layer UUIDs, canvas dimensions, resolution, active layer, guides and layers in bottom-to-top order. Layers retain visibility, hierarchy, opacity, blend mode, transforms, masks, clipping references and applicable editable text, shape, adjustment or effect settings.

Assets are named `<LAYER-UUID>.png` and `<LAYER-UUID>.mask.png`, using uppercase UUIDs. Blank layers have no image asset. Image pixels and transforms are stored separately, so moving or deleting an imported source file does not break the project. Masks use grayscale coverage: white reveals and black hides. `maskPlacement` and `maskLinked` retain independent mask placement and linking.

Editable text and shapes include cached raster pixels. Text metadata stores content, font name, size, RGB color, alignment, tracking, leading and optional paragraph bounds. Cached pixels preserve the saved appearance without requiring the original font; editing text may render differently on another platform. Effects include stroke, shadow, color overlay, inner shadow, outer glow and inner glow.

Undo history, pixel selections, viewport, group collapse state and view preferences are not serialized. Opening starts a new history and fits the canvas. PNG/JPEG exports are flattened derivatives and do not mark project edits as saved.

## Version changes

| Version | Added schema support |
| --- | --- |
| 1 | Canvas, raster layers, transforms and embedded images. Optional `resolution` defaults to 72 pixels/inch. |
| 2 | Layer hierarchy through `parentID` and `isGroup`. |
| 3 | Layer `opacity` and `blendMode`. |
| 4 | Raster layer `maskFile` and `maskEnabled`. |
| 5 | Clipping references through `maskSourceID`. |
| 6 | Masks on groups. |
| 7 | Adjustment layers. The schema also accepts additive editable shape, text and effect metadata. |
| 8 | Document guides and non-default folder opacity. Older Linux v7 files with folder opacity remain readable. |
| 9 | Gaussian Blur, Motion Blur and Add Noise adjustment layers. |
| 10 | Per-character text colors through `text.colorRuns`. |

Color runs contain `location`, `length`, `red`, `green` and `blue`. Locations and lengths count **UTF-16 code units**, not UTF-8 bytes. Runs must be ordered, non-overlapping and nonempty, and cannot split a surrogate pair. Uncovered text uses the layer's base color.

Guides contain `id`, `axis` (`horizontal` or `vertical`) and a finite document-pixel `position`. Guide visibility, locking, grid and snapping are application preferences stored separately from the project.

Groups are pass-through: group opacity and masks multiply descendant coverage. A clipping reference uses its source's alpha, transform, opacity and masks; source visibility and RGB do not contribute to clipping coverage. Cycles and invalid references are rejected.

## Validation and saving

Linux validates metadata, hierarchy, asset names and allocation limits before replacing the open document. Unknown manifest fields, unsupported versions, missing assets and unsafe paths are rejected. Saves stage a complete package before atomic directory replacement.

| Limit | Linux value |
| --- | --- |
| Canvas or image side | 30,000 pixels |
| One materialized raster surface | 200 million pixels |
| Combined document image and mask pixels | Physical RAM bytes ÷ 16, clamped to 200–800 million pixels |
| Layers | 10,000 |
| Ancestor depth | 64 |
| Guides | 1,000, with positions within ±1,000,000 pixels |
| Manifest | 4 MiB |
| Each encoded image or mask asset | 512 MiB |
| Resolution | 1–9,600 pixels/inch |

Sparse canvas dimensions are independent of the materialized-pixel budget. The document budget defaults to 200 million pixels if physical RAM cannot be determined. Other implementations may impose different limits.

## Linux RAW extension

RAW layers retain their developed PNG in the ordinary manifest. The optional `linux-raw.json` sidecar (version 1) stores layer/source UUIDs, camera metadata and development settings. Source bytes reside in `raw/<source-UUID>.raw`. Duplicated layers can share one source while retaining independent development settings.

Unique RAW sources share a 512 MiB limit; the sidecar has a 4 MiB limit. The loader validates references, settings and source paths. RAW data is staged with the rest of the package during saving.

The upstream macOS writer preserves the developed pixels but discards this extension when resaving. Keep a Linux copy to retain RAW editability. See [RAW editing](linux-raw.md).
