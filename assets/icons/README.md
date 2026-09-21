# Interface icons

Lucide icons, pinned to commit `ba6751ac45d379f6359d75ef2228cff6ba96c122` of https://github.com/lucide-icons/lucide.
See LICENSE for the ISC license. QuickGUI embeds and renders the SVG glyphs.

`trash-2.svg` retains the local semantic name for upstream `trash.svg`.

The `compositor-*` glyphs are project assets. Curves, exposure, grain, and hue-strip markers follow their Swift controls. The gradient ports the dither pattern in `Compositor/UI/GradientControls.swift`; the mask and adjustment glyphs follow the original layer footer.

The clone stamp and polygonal lasso port the paths in `BrushControls.swift` and `LassoControls.swift`.

Zoom uses Lucide's `search` glyph. The rail sets visible
glyph sizes individually because a 17-point SF Symbols font and a 17-pixel SVG
box do not produce equal icon bounds.

The native workspace reference guides `compositor-move.svg` (separate diagonal
arrows), `compositor-marquee.svg` (a dashed rectangle),
`compositor-healing.svg` (a diagonal rounded bandage), and
`compositor-shapes.svg` (a square in front of a circle on its upper left).
These are project-drawn Linux substitutes, not Apple's SF Symbols.

`compositor-pointed-brush.svg` uses a tapered handle and rounded bristles.
`compositor-hand.svg` uses the drawing gesture and motion arc from the native
Hand tool's motif. `compositor-eyedropper.svg` uses a filled bulb above the
outlined tube, shared by the rail and adjustment sampling buttons.

Hue's targeted adjustment uses Lucide's `pointer` glyph, distinct from the open
hand gesture used for canvas navigation.

`compositor-progress.svg` is a twelve-spoke progress indicator for active jobs.

`compositor-popup-chevron.svg` follows the paired arrows in the native Blend picker reference.

`compositor-mask.svg` uses the source's inset rectangle. `compositor-folder-plus.svg` places the plus in a separate badge, matching the source folder control.

`compositor-levels-handle-shadow.svg` supplies the Levels triangles’ half-point gray shadow as a cached alpha mask. The triangle uses the same local path as `levels_handles.rs`; separating its mask avoids window-sized compositing textures for tiny control shadows.
