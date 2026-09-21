//! ShapeControls.swift's direct shape, rectangle radius and fill controls.
use super::scalar_controls::Scalar;
use super::*;

impl Editor {
    pub(super) fn set_shape_kind(&mut self, ellipse: bool) {
        if matches!(self.gesture, Some(Gesture::Shape(_))) {
            self.gesture = None;
            self.session_mut().cancel();
        }
        self.tools.shape_ellipse = ellipse;
    }

    pub(super) fn shape_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut modes = div()
            .flex_row()
            .gap(2.)
            .p(2.)
            .rounded(6.)
            .bg(Color::rgb8(29, 29, 29))
            .flex_shrink_0();
        for (ellipse, label) in [(false, "Rectangle"), (true, "Ellipse")] {
            modes = modes.child(
                Self::segment(label, self.tools.shape_ellipse == ellipse)
                    .tooltip("Shift-U switches between Rectangle and Ellipse")
                    .on_click(cx.listener(format!("shape-{label}"), move |this, cx| {
                        this.set_shape_kind(ellipse);
                        this.status = this.tool_hint().into();
                        cx.invalidate();
                    })),
            );
        }
        let mut row = div().flex_row().items_center().gap(12.).child(modes);
        if !self.tools.shape_ellipse {
            row = row.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(6.)
                    .flex_shrink_0()
                    .tooltip(
                        "Round the rectangle's corners by this many pixels; 0 keeps them square",
                    )
                    .child(text("Radius").text_size(12.).line_height(15.))
                    .child(self.scalar_slider(
                        cx,
                        "shape-radius-slider",
                        "Radius",
                        Scalar::ShapeRadius,
                        (0., 200.),
                        100.,
                    ))
                    .child(Self::unit_suffix(
                        self.brush_value(cx, "shape-radius", Scalar::ShapeRadius, (0., 5000.))
                            .text_right()
                            .w(48.),
                        "px",
                    )),
            );
        }
        row.child(self.header_foreground(cx, palette_controls::ForegroundStyle::Shape))
    }
}
