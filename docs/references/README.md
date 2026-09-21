# macOS visual references

[macOS typography specifications](macos-typography.md) record Apple's text-style sizes used to interpret the Swift controls.

Three public upstream screenshots were retrieved on 2026-09-19:

| Capture | Source | Scope |
| --- | --- | --- |
| [Workspace](macos-upstream-workspace.png) | [PR 27](https://github.com/robbietilton/Compositor/pull/27), [original image](https://github.com/user-attachments/assets/5669cd8a-2589-4777-a542-24c461e88438) | Existing toolbar, tool rail, Layers controls and footer. The proposed Text tool and text layers are outside the source revision being ported. |
| [New canvas](macos-upstream-new-canvas.png) | [PR 28](https://github.com/robbietilton/Compositor/pull/28), [original image](https://github.com/user-attachments/assets/d819ef8a-dc98-4a6e-ada0-80d9ee9df410) | Existing fields, typography and buttons. The added clipboard button is outside the source revision being ported. |
| [File menu](macos-upstream-file-menu.png) | [PR 28](https://github.com/robbietilton/Compositor/pull/28), [original image](https://github.com/user-attachments/assets/73d395ee-a1c6-4748-9089-0aefc747a9f7) | Existing menu typography, inset and initial bar order. The added clipboard command is outside the source revision being ported. |

Both PRs use base commit `a19db9011282399785dc18efcfded904627bdcc2`, which is this checkout's original Swift revision. At retrieval, both were open and unmerged. The PR 27 diff adds the Text header and its status hint in `ContentView`; it does not change the existing rail styling, toolbar or Layers appearance.

The original PNGs retain their embedded `DELL G3223Q` color profile. Convert through that profile to sRGB before comparing pixel colors with Linux captures. Merely calling Pillow's `convert("RGB")` does not perform this conversion. Use `ImageCms.profileToProfile` with the embedded profile and `ImageCms.createProfile("sRGB")`.

These captures guided corrections to slider geometry, accent color, primary buttons, selected-layer treatment, menu order and text sizing. Font metrics and native materials remain comparison limits. The File menu crop places View immediately after Edit and uses a 14–15-pixel command-text inset. The PR changes only add the clipboard command, so the visible order belongs to the existing native menu structure. The crop does not show the entire menu bar, and no native adjustment-panel capture is available. The older `levels.png`, `hue-saturation.png` and `canvas-size.png` images have different control arrangements from the current Swift implementation and are not evidence of its rendered appearance.

The Canvas Size and JPEG sheets call Foundation's `ByteCountFormatter`. Its [documented defaults and open implementation](https://github.com/swiftlang/swift-corelibs-foundation/blob/main/Sources/Foundation/ByteCountFormatter.swift), inspected on 2026-09-20, specify decimal File units, binary Memory units, and adaptive precision: no decimals for bytes/KB, at most one for MB and two for larger units, without zero padding. [NumberFormatter](https://github.com/swiftlang/swift-corelibs-foundation/blob/main/Sources/Foundation/NumberFormatter.swift) defaults to half-even rounding. Linux applies these rules to its English UI and uses the source sheet's primary byte-count text beside its secondary preview note. These references establish the formatting rules, not current native macOS font rasterization or localization equivalence.

The upstream [Actions runs](https://api.github.com/repos/robbietilton/Compositor/actions/runs) and [artifacts](https://api.github.com/repos/robbietilton/Compositor/actions/artifacts) APIs were checked on 2026-09-20 for additional native reference captures. They listed one completed Copilot code-review run at this source revision and zero artifacts. The checked-in XCTest screenshot covers the editor foundation, not the adjustment panels; no additional native panel image was obtained from CI.
