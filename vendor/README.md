# QuickGUI Linux integration patch

`quickgui/` contains the published QuickGUI 0.1.5 crate from
<https://github.com/egoist/quickgui>, commit
`d327e10b214596afbd6b830f0cc6934622320913`.
Cargo uses this pinned copy through `[patch.crates-io]`.

Local changes:

- `src/runtime/window_creation.rs` passes `AppInfo` to Winit's
Linux window-name attributes before creating the window. This sets the
Wayland app ID and X11 window class, allowing desktop icons, launchers,
and window rules to identify Compositor. Compositor's identifier matches
the installed `compositor.desktop` filename.

- `src/clipboard.rs` adds an application clipboard provider shared by native
  text controls and explicit commands, guarded against reentrant access.
- `src/runtime/external.rs` exposes an owned display handle and provider setter;
  `src/lib.rs` exports the provider trait. Compositor uses the window's Wayland
  connection for standard `wl_data_device` access, including GNOME without
  data-control. The owned handle keeps the connection alive during transfers.

- `src/select.rs` adds `SelectState::with_opaque_popover_background`, an opt-in
  background for native select windows. The upstream transparent surface fails
  on X11/Vulkan adapters without alpha-capable window surfaces. Compositor's size
  and crop dropdowns use this option; the upstream default stays transparent.

- `src/runtime/window_creation.rs` creates X11 system popovers as separate
  `PopupMenu` windows. Winit's `parent_window` embeds an X11 child, causing
  focus/dismissal failures and using the wrong coordinate space for a popup.
  Other native backends retain their existing parent-window setup.
- `src/runtime/event_loop_window.rs` ignores synthetic key presses replayed
  by Winit after focus changes. Otherwise the Return that commits a select
  reactivates its owner and immediately reopens it. Synthetic releases still
  reset held-key state. Native X11 checks under Openbox cover keyboard opening,
  selection, Escape, mouse selection, and restored parent focus.

- Text inputs accept opt-in `text_input_padding` for compact numeric fields.
  Painting, selection, caret geometry, and horizontal scrolling share the same
  inset. The default remains 12 pixels; editor decorations retain their own
  padding.

- `Element::disable_subtree` explicitly disables an already-built control tree.
  The upstream `disabled` builder affects only the element itself. Compositor uses
  the opt-in subtree operation while a floating color/adjustment picker samples
  the canvas, preventing edits through toolbar and layer-panel children.

- Tooltip reconciliation only uses keyboard-visible focus as a fallback after
  pointer exit. Mouse focus previously reopened a clicked button's tooltip when
  the pointer moved away. `tests/tooltip_focus.rs` in the application exercises
  rendered pointer-exit, hover, and keyboard-focus behavior. The published crate
  omits font fixtures needed to compile its own unit-test target.

- `src/runtime/wayland_activation.rs` handles explicit focus requests using
  `xdg_activation_v1`. Winit's Wayland `focus_window` is a no-op. One worker
  reuses a registry, obtains the focused seat's keyboard serial, and activates
  the requested surface. Requests retain both native windows, use a bounded
  queue and token timeout, and release temporary protocol objects. The runtime
  waits for the compositor's focus event before changing its active window.
  Native Niri testing exposed and verifies reopening an existing About window.

- Group focus styling follows keyboard-visible focus, including descendant
  styles, without adding focus scopes to the tab order. Regression coverage
  lives in the vendor focus and state-style tests.

- Backdrop masks retain the original element border box independently of
  overflowing paint bounds. Backdrop-only groups share capture/blur textures
  and draw their foreground into the parent, retaining ancestor clipping and
  isolation where blending, filters, opacity or transforms require it. This
  keeps multiple 4K menus within the existing texture budget. Renderer tests
  run on Linux as well as macOS.

- Stroked paths use an inset core and a separate interpolated antialias fringe
  instead of fading inward across each tessellated triangle. Shared boundary
  normals and centerlines preserve width and joins across subdivision,
  reflection and nonuniform scaling. Stroke metadata counts against the path
  byte limit; emitted fringe vertices count against the existing frame cap.
  Paint bounds include the fringe for clipping and overlap ordering. Filled
  paths retain their original coverage. GPU regressions check geometric
  coverage, subdivision, opacity and wider-stroke SVG comparisons.

- Canvas fragments can share a rounded clipping surface through
  `fill_rect_with_rounded_clip`, using the existing quad shader without an
  offscreen group. Local coordinates, ancestor clipping and zero/invalid
  radii have focused Canvas tests. Compositor's Hue spectrum rendering test
  checks transparent corners and retained interior colors.

- Collapsed transforms retain an isolated scene group and produce no composite
  draws or content-texture allocation. Previously a zero scale fell back to
  drawing the original subtree at full size. A renderer regression checks both
  zero-scale axes, collapse/re-expansion and 1×/2× backing scales. The application
  verifies animated tab fades, continuous reversal, reduced motion and 4K output.

Keep the upstream license files and third-party notices with this copy.
Reconcile every local patch against a newer QuickGUI release before removing
the vendor directory, then verify both native backends.
