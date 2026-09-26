# Vendored dependencies

Cargo uses local copies of QuickGUI 0.1.5 and quickgui-winit 0.1.5 through
`[patch.crates-io]`. Both come from [QuickGUI](https://github.com/egoist/quickgui)
commit `d327e10b214596afbd6b830f0cc6934622320913`. The Winit support crate derives
from Winit 0.30.13.

## Local QuickGUI patches

| Area | Purpose |
| --- | --- |
| Window identity | Set the Wayland app ID and X11 class to match `compositor.desktop`. |
| Clipboard | Share the window's Wayland connection between application commands and text controls, including GNOME without data-control. |
| Select popovers | Support opaque surfaces and separate X11 popup windows; ignore synthetic key presses that reopen a committed selection. |
| Text input | Allow compact padding and preserve text selection across color-picker interactions. |
| Disabled controls | Disable entire control trees while a floating picker samples the canvas. |
| Tooltips and focus | Keep pointer focus from reopening tooltips and apply group focus styling only for keyboard-visible focus. |
| Wayland activation | Use `xdg_activation_v1` for explicit focus requests and wait for the compositor's focus event. |
| Backdrop rendering | Retain border-box masks and share capture/blur textures without losing clipping or isolation. |
| Path rendering | Preserve stroke width and joins with a separate antialias fringe. |
| Canvas clipping | Provide `fill_rect_with_rounded_clip` without an offscreen group. |
| Collapsed transforms | Draw no content for zero-scale transforms. |
| Tablet input | Carry pressure, tilt and eraser metadata through normal pointer dispatch and capture. |

## Local Winit patches

Tablet frames use the existing Wayland tablet-v2 and XInput2 event streams,
replacing mouse emulation to avoid duplicate strokes. XInput axes use
driver-provided labels and ranges. Wayland handles per-surface routing and pen
cursors, with a fallback when cursor-shape-v1 is unavailable. Device removal and
proximity loss release held buttons.

## Updating these dependencies

Reconcile each local patch before updating the pinned crates, then verify both
Wayland and X11. Preserve the upstream license files and third-party notices:
QuickGUI is MIT OR Apache-2.0, and quickgui-winit is Apache-2.0. Winit's Markdown
changelogs are included by its Rust source and must remain available to build.
