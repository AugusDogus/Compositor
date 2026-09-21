# Linux UI completion

This closes the UI work for the macOS 1.0.4 baseline. Newer upstream feature gaps are tracked in [Linux feature status](linux-features.md).

The visual goal is complete under the user's clarified target: preserve the
macOS application's functionality and deliver a polished Linux UI. Exact visual
matching is not required. Font substitution and approximate AppKit materials
are platform differences, not unfinished requirements.

This file previously tracked remaining visual questions. The long chronology in
[linux-port.md](linux-port.md) records completed increments, not a queue of
surfaces to audit again.

## Existing evidence to reuse

- [UI inventory](linux-port.md#ui-acceptance-inventory): mappings for the 28 Swift UI
  files, workspace, application commands and custom canvas overlays.
- [Native review set](../dist/screenshots/README.md): tool states, menus, panels,
  dialogs, cancellation and preservation checks on X11 and nested Wayland.
- [Latest capture manifest](../dist/screenshots/linux-minimum-window.json): the
  581-test executable and minimum-window checks. The broader review set retains
  its separately recorded 579-test executable.
- [Mac reference provenance](references/README.md): three public captures from
  the source revision, covering workspace, New Canvas and File menu.
- [Current side-by-side review](../dist/screenshots/linux-macos-current-review.png):
  original-scale crops of the existing 581-test Linux captures and color-managed
  Mac references. This supplies a concrete current appearance review without
  repeating implementation checks or claiming coverage beyond those references.

The defects recorded through the minimum-window increment have fixes and
verification. The current portable executable matches the binary used in native
checks. Its existing results are 581 ordinary tests passed, 12 opt-in tests
ignored, formatting passed, application Clippy passed and release packaging
passed. This closeout changed documentation only; it did not repeat those runs.

## Completion evidence

| Requirement | Evidence | Result |
| --- | --- | --- |
| Icons and workspace chrome | Source motifs and public Mac references guided the custom/Lucide glyphs, typography, rail, toolbar and Layers controls. The current comparison contains native Linux crops. | Implemented and visually reviewed. Inter and Linux glyph artwork are retained. |
| Menus and command structure | Source command mappings, menu regressions and native captures cover all eight menus, shortcuts, disabled states, checkmarks and submenus. | Implemented and verified. Linux places its menus inside the window. |
| Panels, controls and dialogs | The UI inventory maps all 28 Swift UI files. The native review set covers 21 tool states, six adjustments, five filters, size/export sheets, color picking and New Canvas. Specific interaction regressions and native checks cover previews, commit, cancellation and Undo. | Implemented and verified on Linux. Exact AppKit materials are not required. |
| Responsive layout and usability | The current executable has minimum-window checks on X11 and Wayland, including scrolling controls, palette access, fixed status bar, single-line New Canvas actions and 37 pixel-preservation comparisons per backend. | Verified at normal and minimum sizes. |
| Real application evidence and delivery | Screenshot manifests record executable hashes. The standalone binary and archive match the native-tested executable. The screenshot index distinguishes the broad 579-test capture set from the newer 581-test minimum-window set. | Verified; no mockups substitute for application screenshots. |

This closes the visual goal, not a claim that every result is bit-identical to
macOS or that every possible edge case has been tested. Functional and platform
differences, including the requested BiRefNet replacement for Apple Vision, are
recorded in [the implementation notes](linux-port.md#parity-acceptance-and-platform-differences).

## Future changes

- Do not restart the broad source audit or recapture the entire UI without new
  evidence that warrants it.
- Start implementation work from a named discrepancy, its source/reference and
  the expected visible result. Record the specific acceptance check.
- Reuse previous test and capture evidence for unchanged behavior. Run the
  smallest relevant checks for a change; perform a full release check when
  preparing a deliverable, not as a substitute for a parity decision.
- Preserve the clarified target. Do not reopen this goal solely to reproduce
  Apple's font rasterization, native materials or OS decoration.
