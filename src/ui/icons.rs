//! Embedded glyphs share a parsed vector asset at every display scale.
use super::*;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Move,
    SquareDashed,
    CircleDashed,
    Lasso,
    LassoSelect,
    WandSparkles,
    Crop,
    Paintbrush,
    Eraser,
    Stamp,
    Bandage,
    Droplet,
    Gradient,
    Shapes,
    Pipette,
    Hand,
    ZoomIn,
    ZoomOut,
    Plus,
    X,
    ChevronRight,
    ChevronDown,
    Eye,
    EyeOff,
    Folder,
    FolderPlus,
    SquarePlus,
    Trash2,
    Circle,
    Link,
    Unlink,
    ArrowLeftRight,
    Settings2,
    Check,
    Mask,
    Adjustment,
    Curves,
    Exposure,
    Grain,
    Palette,
    HueMarkers,
    RotateCcw,
    Layers,
    Zoom,
    Pointer,
    Progress,
    PopupChevron,
}
impl Icon {
    fn svg(self) -> quickgui::Svg {
        static ICONS: OnceLock<[quickgui::Svg; 47]> = OnceLock::new();
        ICONS.get_or_init(|| {
            [
                include_bytes!("../../assets/icons/compositor-move.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-marquee.svg").as_slice(),
                include_bytes!("../../assets/icons/circle-dashed.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-lasso.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-polygonal-lasso.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-wand.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-crop.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-pointed-brush.svg").as_slice(),
                include_bytes!("../../assets/icons/eraser.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-clone-stamp.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-healing.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-drop.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-gradient.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-shapes.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-eyedropper.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-hand.svg").as_slice(),
                include_bytes!("../../assets/icons/zoom-in.svg").as_slice(),
                include_bytes!("../../assets/icons/zoom-out.svg").as_slice(),
                include_bytes!("../../assets/icons/plus.svg").as_slice(),
                include_bytes!("../../assets/icons/x.svg").as_slice(),
                include_bytes!("../../assets/icons/chevron-right.svg").as_slice(),
                include_bytes!("../../assets/icons/chevron-down.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-eye.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-eye-slash.svg").as_slice(),
                include_bytes!("../../assets/icons/folder.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-folder-plus.svg").as_slice(),
                include_bytes!("../../assets/icons/square-plus.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-trash.svg").as_slice(),
                include_bytes!("../../assets/icons/circle.svg").as_slice(),
                include_bytes!("../../assets/icons/link.svg").as_slice(),
                include_bytes!("../../assets/icons/unlink.svg").as_slice(),
                include_bytes!("../../assets/icons/arrow-left-right.svg").as_slice(),
                include_bytes!("../../assets/icons/settings-2.svg").as_slice(),
                include_bytes!("../../assets/icons/check.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-mask.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-adjustment.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-curves.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-exposure.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-grain.svg").as_slice(),
                include_bytes!("../../assets/icons/palette.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-hue-markers.svg").as_slice(),
                include_bytes!("../../assets/icons/rotate-ccw.svg").as_slice(),
                include_bytes!("../../assets/icons/layers.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-magnifyingglass.svg").as_slice(),
                include_bytes!("../../assets/icons/pointer.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-progress.svg").as_slice(),
                include_bytes!("../../assets/icons/compositor-popup-chevron.svg").as_slice(),
            ]
            .map(|bytes| quickgui::Svg::from_bytes(bytes).expect("Embedded icon must be valid SVG"))
        })[self as usize]
            .clone()
    }
    pub fn element(self, size: f32) -> Element {
        quickgui::svg(self.svg())
            .w(size)
            .h(size)
            .flex_shrink_0()
            .accessibility_hidden(true)
    }
    pub fn button(self, label: impl Into<Arc<str>>) -> Element {
        self.button_with_icon_size(
            label,
            match self {
                Self::Stamp | Self::LassoSelect | Self::Gradient => 18.,
                Self::Eye | Self::EyeOff => 20.,
                Self::SquarePlus => 14.,
                _ => 17.,
            },
        )
    }
    pub fn button_with_icon_size(self, label: impl Into<Arc<str>>, size: f32) -> Element {
        let label = label.into();
        button()
            .w(32.)
            .h(30.)
            .p(0.)
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .justify_center()
            .rounded(6.)
            .bg(Color::TRANSPARENT)
            .text_color(Color::rgb8(210, 210, 210))
            .hover(|s| s.bg(Color::rgb8(66, 66, 66)))
            .focus(super::controls::focus_outline)
            .disabled_style(|s| s.opacity(0.4))
            .accessibility_label(label.clone())
            .tooltip(label)
            .child(self.element(size))
    }
}
impl Editor {
    pub(super) fn icon_action(
        &self,
        cx: &mut ViewContext<'_, Self>,
        id: u64,
        icon: Icon,
        label: &'static str,
        action: Action,
    ) -> Element {
        icon.button(label)
            .on_click(cx.listener(id, move |this, cx| this.action(action, cx)))
    }
}

/// Animate only while a job's progress indicator is mounted.
pub(super) fn progress(id: &'static str, size: f32) -> Element {
    use quickgui::{Animation, AnimationExt, Progress};
    let glyph = Icon::Progress.element(size).with_animation(
        id,
        Animation::new(std::time::Duration::from_secs(1))
            .repeat()
            .with_max_fps(12.),
        |element, phase| element.rotate_degrees((phase * 12.).floor() * 30.),
    );
    Progress::indeterminate().root_with(div().id(id).accessibility_label("Processing").child(glyph))
}
