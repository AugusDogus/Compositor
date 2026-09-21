//! Error presentation is independent of edit forms, so acknowledging a failure
//! cannot apply or cancel the adjustment underneath it.
use super::*;
use quickgui::Dialog;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Paint,
    Crop,
    Import,
    Open,
    Save,
    ExportPng,
    ExportTiff,
    ExportWebp,
    RawDevelop,
    ExportPsd,
    ExportJpeg,
    CanvasSize,
    ImageSize,
    Clipboard,
    CreateCanvas,
    About,
}

impl Operation {
    pub(super) fn for_action(action: Action) -> Self {
        match action {
            Action::New => Self::Clipboard,
            Action::Open | Action::OpenPsd | Action::OpenRaw => Self::Open,
            Action::Import => Self::Import,
            Action::Save | Action::SaveAs => Self::Save,
            Action::ExportPng => Self::ExportPng,
            Action::ExportTiff => Self::ExportTiff,
            Action::ExportWebp => Self::ExportWebp,
            Action::ExportPsd => Self::ExportPsd,
            Action::ExportJpeg | Action::ExportJpegFile => Self::ExportJpeg,
            Action::CanvasSize => Self::CanvasSize,
            Action::ImageSize => Self::ImageSize,
            _ => Self::Paint,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Paint => "Couldn’t paint",
            Self::Crop => "Couldn’t crop",
            Self::Import => "Import couldn’t finish",
            Self::Open => "Couldn’t open the project",
            Self::Save => "Couldn’t save the project",
            Self::ExportPng => "Couldn’t export PNG",
            Self::ExportTiff => "Couldn’t export TIFF",
            Self::ExportWebp => "Couldn’t export WebP",
            Self::RawDevelop => "RAW development is still open",
            Self::ExportPsd => "Couldn’t export PSD",
            Self::ExportJpeg => "Couldn’t export JPEG",
            Self::CanvasSize => "Couldn’t change canvas size",
            Self::ImageSize => "Couldn’t resize the image",
            Self::Clipboard => "Couldn’t read the clipboard",
            Self::CreateCanvas => "Couldn’t create the canvas",
            Self::About => "Couldn’t open About Compositor",
        }
    }
}

pub(super) struct Failure {
    pub(super) operation: Operation,
    pub(super) message: String,
}

impl Editor {
    pub(super) fn show_error(&mut self, operation: Operation, message: impl Into<String>) {
        let message = message.into();
        self.status.clone_from(&message);
        self.errors.push_back(Failure { operation, message });
    }

    pub(super) fn operation_result(
        &mut self,
        operation: Operation,
        result: Result<()>,
        cx: &mut EventContext,
    ) {
        if let Err(error) = result {
            self.show_error(operation, error.to_string());
        }
        self.changed(cx);
    }

    fn dismiss_error(&mut self, cx: &mut EventContext) {
        self.errors.pop_front();
        cx.invalidate();
    }

    pub(super) fn error_view(&mut self, cx: &mut ViewContext<'_, Self>) -> Option<Element> {
        let failure = self.errors.front()?;
        let dialog = Dialog::alert("operation-error", true).initial_focus("error-ok");
        let contents = Self::alert_contents(
            dialog,
            failure.operation.title(),
            failure.message.clone(),
            cx.size().height,
        )
        .child(
            Self::alert_button("OK")
                .bg(Color::rgb8(0, 122, 255))
                .hover(|s| s.bg(Color::rgb8(24, 137, 255)))
                .on_click(cx.listener("error-ok", |this, cx| this.dismiss_error(cx))),
        );
        let dismiss = cx.dismiss_listener(dialog.popover_id(), |this, cx| this.dismiss_error(cx));
        Some(
            dialog
                .root()
                .flex_row()
                .items_center()
                .justify_center()
                .child(dialog.backdrop().bg(Color::TRANSPARENT))
                .child(
                    dialog
                        .popup_with(contents)
                        .on_dismiss(dismiss)
                        // Keep keyboard shortcuts out of the underlying panel.
                        // Button activation and topmost Escape dismissal retain
                        // QuickGUI's default handling.
                        .on_key_down(cx.key_down_listener(dialog.popover_id(), |_, _, cx| {
                            cx.stop_propagation();
                        })),
                ),
        )
    }

    pub(super) fn alert_contents(
        dialog: Dialog,
        title: impl Into<Arc<str>>,
        description: impl Into<Arc<str>>,
        height: f32,
    ) -> Element {
        let mut contents = div()
            .w(340.)
            .max_h((height - 80.).max(200.))
            .overflow_y_scroll()
            .p(24.)
            .gap(16.)
            .flex_col()
            .items_center()
            .bg(Color::rgb8(45, 45, 45))
            .border(1., Color::rgb8(90, 90, 90))
            .rounded(12.)
            .shadow(super::surfaces::panel_shadow());
        contents = match super::about::app_icon() {
            Ok(icon) => contents.child(
                quickgui::img(icon)
                    .w(64.)
                    .h(64.)
                    .rounded(14.)
                    .overflow_hidden()
                    .flex_shrink_0(),
            ),
            Err(error) => contents.child(text(error.to_string()).text_size(11.).wrap()),
        };
        contents
            .child(
                dialog.title_with(
                    text(title)
                        .text_size(13.)
                        .font_semibold()
                        .text_center()
                        .wrap(),
                ),
            )
            .child(dialog.description_with(text(description).text_size(12.).text_center().wrap()))
    }

    pub(super) fn alert_button(label: &'static str) -> Element {
        Self::control(label)
            .w_full()
            .h(30.)
            .rounded(6.)
            .flex_row()
            .items_center()
            .justify_center()
    }
}
