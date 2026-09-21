//! LassoControls.swift: selection modes and direct expand/contract amounts.
use super::scalar_controls::Scalar;
use super::*;
use compositor::selection::SelectionMode;

impl Editor {
    pub(super) fn object_edge_control(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div().flex_row().items_center().gap(6.).flex_shrink_0()
            .tooltip("Adjust each detected object before adding or subtracting it: positive contracts, negative expands")
            .child(Self::tool_header_control("Edge"))
            .child(Self::unit_suffix(self.brush_value(cx, "object-edge-offset", Scalar::ObjectEdge, (-10., 10.)).text_right().w(40.), "px"))
    }
    pub(super) fn displayed_selection_mode(&self) -> SelectionMode {
        if let Some(draft) = &self.tools.polygon {
            return draft.mode;
        }
        if let Some(
            canvas::Gesture::Region { mode, .. }
            | canvas::Gesture::Lasso { mode, .. }
            | canvas::Gesture::Object { mode, .. },
        ) = &self.gesture
        {
            return *mode;
        }
        canvas::selection_mode(self.keyboard_modifiers, self.tools.selection_mode)
    }
    pub(super) fn selection_modify_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut row = div().flex_row().items_center().gap(12.).flex_1().child(
            div()
                .w(1.)
                .h(18.)
                .bg(Color::rgb8(72, 72, 72))
                .flex_shrink_0(),
        );
        for (expand, label, id, scalar) in [
            (
                true,
                "Expand",
                "selection-expand-amount",
                Scalar::SelectionExpand,
            ),
            (
                false,
                "Contract",
                "selection-contract-amount",
                Scalar::SelectionContract,
            ),
        ] {
            let disabled = !self.can_modify_selection();
            let mut control = div()
                .flex_row()
                .items_center()
                .gap(6.)
                .flex_shrink_0()
                .tooltip(format!("{label} the selection by this many pixels"))
                .child(Self::tool_header_control(label).on_click(cx.listener(
                    if expand { 413_u64 } else { 414_u64 },
                    move |this, cx| {
                        let amount = if expand {
                            this.tools.selection_expand_amount
                        } else {
                            this.tools.selection_contract_amount
                        };
                        let result = this.resize_selection(expand, amount);
                        this.operation_result(alerts::Operation::Paint, result, cx);
                    },
                )))
                .child(Self::unit_suffix(
                    self.brush_value(cx, id, scalar, (1., 500.))
                        .text_right()
                        .w(40.),
                    "px",
                ));
            if disabled {
                control.disable_subtree();
                control = control.opacity(0.45);
            }
            row = row.child(control);
        }
        let mut feather = div()
            .flex_row()
            .items_center()
            .gap(6.)
            .flex_shrink_0()
            .child(Self::tool_header_control("Feather").on_click(cx.listener(
                "selection-feather",
                |this, cx| {
                    let result = this.feather_selection(this.tools.selection_feather_amount);
                    this.operation_result(alerts::Operation::Paint, result, cx);
                },
            )))
            .child(Self::unit_suffix(
                self.brush_value(
                    cx,
                    "selection-feather-amount",
                    Scalar::SelectionFeather,
                    (1., 250.),
                )
                .text_right()
                .w(40.),
                "px",
            ));
        if !self.can_modify_selection() {
            feather.disable_subtree();
            feather = feather.opacity(0.45);
        }
        row = row.child(feather).child(div().flex_1());
        if let Some(selection) = self
            .current_document()
            .and_then(|doc| doc.selection.as_ref())
        {
            if selection.bounds().is_none() {
                row = row.child(
                    text("Empty selection")
                        .text_size(12.)
                        .line_height(15.)
                        .text_color(Color::rgb8(160, 160, 160))
                        .whitespace_nowrap(),
                );
            }
            row = row.child(
                Self::tool_header_control("Deselect")
                    .disabled(!self.can_edit_layers())
                    .on_click(cx.listener("header-deselect", |this, cx| {
                        this.action(Action::Deselect, cx)
                    })),
            );
        }
        row
    }
    pub(super) fn selection_mode_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let active = self.displayed_selection_mode();
        let mut row = div()
            .flex_row()
            .gap(2.)
            .p(2.)
            .rounded(6.)
            .bg(Color::rgb8(29, 29, 29))
            .flex_shrink_0();
        for (mode, label) in [
            (SelectionMode::Replace, "New"),
            (SelectionMode::Add, "Add"),
            (SelectionMode::Subtract, "Subtract"),
        ] {
            let id = format!("selection-mode-{label}");
            row = row.child(
                Self::segment(label, mode == active)
                    .tooltip("Hold Shift to add or Alt to subtract for one outline")
                    .on_click(cx.listener(id, move |this, cx| {
                        this.tools.selection_mode = mode;
                        cx.invalidate();
                    })),
            );
        }
        row
    }
}
