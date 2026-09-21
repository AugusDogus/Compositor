//! Depth for in-window replacements of AppKit menus and floating panels.
use quickgui::{BoxShadow, Color};

pub(super) fn menu_shadow() -> BoxShadow {
    BoxShadow::new(0., 5., Color::rgba8(0, 0, 0, 110)).blur_radius(14.)
}

pub(super) fn panel_shadow() -> BoxShadow {
    BoxShadow::new(0., 10., Color::rgba8(0, 0, 0, 128)).blur_radius(28.)
}
