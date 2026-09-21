//! ShapeControls.swift's direct shape, rectangle radius and fill controls.
use super::scalar_controls::Scalar;
use super::*;
use compositor::document::ShapeKind;

impl Editor {
    pub(super) fn set_shape_kind(&mut self, kind: ShapeKind) {
        if matches!(self.gesture, Some(Gesture::Shape(_))) {
            self.gesture = None;
            self.session_mut().cancel();
        }
        self.tools.shape_kind = kind;
    }

    pub(super) fn shape_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut modes = div()
            .flex_row()
            .gap(2.)
            .p(2.)
            .rounded(6.)
            .bg(Color::rgb8(29, 29, 29))
            .flex_shrink_0();
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse, ShapeKind::Line] {
            let label = kind.label();
            modes = modes.child(
                Self::segment(label, self.tools.shape_kind == kind)
                    .tooltip("Shift-U cycles Rectangle, Ellipse, and Line")
                    .on_click(cx.listener(format!("shape-{label}"), move |this, cx| {
                        this.set_shape_kind(kind);
                        this.status = this.tool_hint().into();
                        cx.invalidate();
                    })),
            );
        }
        let mut row = div().flex_row().items_center().gap(12.).child(modes);
        if self.tools.shape_kind == ShapeKind::Rectangle {
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
        if self.tools.shape_kind == ShapeKind::Line {
            row = row.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(6.)
                    .child(text("Width").text_size(12.))
                    .child(Self::unit_suffix(
                        self.brush_value(
                            cx,
                            "shape-line-width",
                            Scalar::ShapeLineWidth,
                            (1., 5000.),
                        )
                        .w(56.),
                        "px",
                    )),
            );
        }
        row.child(self.header_foreground(cx, palette_controls::ForegroundStyle::Shape))
    }
}
