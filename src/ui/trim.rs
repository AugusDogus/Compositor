use super::*;
use compositor::trim::{Basis, Options};
impl Editor {
    pub(super) fn submit_trim(&mut self, cx: &mut EventContext) {
        let Some(Form::Trim(options)) = self.modal else {
            return;
        };
        if !options.edges.contains(&true) {
            return;
        }
        self.modal = None;
        self.queue(jobs::Job::Trim(options));
        self.changed(cx);
    }
    pub(super) fn trim_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        options: Options,
    ) -> Element {
        let mut content = div().flex_col().gap(12.).child(text("Based On"));
        for (basis, label) in [
            (Basis::Transparent, "Transparent Pixels"),
            (Basis::TopLeft, "Top Left Pixel Color"),
            (Basis::BottomRight, "Bottom Right Pixel Color"),
        ] {
            content = content.child(
                Self::segment(label, options.basis == basis)
                    .id(format!("trim-basis-{label}"))
                    .on_click(cx.listener(format!("trim-basis-{label}"), move |this, cx| {
                        if let Some(Form::Trim(o)) = &mut this.modal {
                            o.basis = basis;
                        }
                        cx.invalidate();
                    })),
            );
        }
        content = content.child(Self::divider()).child(text("Trim Away"));
        for (i, label) in ["Left", "Top", "Right", "Bottom"].into_iter().enumerate() {
            content = content.child(
                Self::check_control(label, options.edges[i])
                    .id(format!("trim-edge-{i}"))
                    .on_click(cx.listener(format!("trim-edge-{i}"), move |this, cx| {
                        if let Some(Form::Trim(o)) = &mut this.modal {
                            o.edges[i] = !o.edges[i];
                        }
                        cx.invalidate();
                    })),
            );
        }
        content.child(
            div()
                .flex_row()
                .gap(8.)
                .child(
                    Self::control("Cancel")
                        .id("trim-cancel")
                        .on_click(cx.listener("trim-cancel", |this, cx| this.cancel_form(cx))),
                )
                .child(div().flex_1())
                .child(
                    Self::control("OK")
                        .id("form-apply")
                        .disabled(!options.edges.contains(&true))
                        .on_click(cx.listener("form-apply", |this, cx| this.submit_trim(cx))),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trim_sheet_defaults_cancel_and_submission() {
        let mut editor = Editor::with_test_document();
        editor.open_form(Action::Trim);
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Trim").size(1200., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.click(window, "trim-basis-Top Left Pixel Color").unwrap();
        cx.click(window, "trim-edge-1").unwrap();
        cx.read(view, |e| {
            let Some(Form::Trim(o)) = e.modal else {
                panic!("Trim sheet missing");
            };
            assert_eq!(o.basis, Basis::TopLeft);
            assert!(!o.edges[1]);
        })
        .unwrap();
        cx.click(window, "trim-cancel").unwrap();
        assert!(
            cx.read(view, |e| e.modal.is_none() && e.job.is_none())
                .unwrap()
        );
        cx.update(view, |e, cx| {
            e.open_form(Action::Trim);
            cx.invalidate();
        })
        .unwrap();
        cx.click(window, "form-apply").unwrap();
        assert!(cx.read(view, |e| e.modal.is_none()).unwrap());
    }
}
