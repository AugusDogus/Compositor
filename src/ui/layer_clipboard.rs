use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Owner {
    #[cfg(test)]
    Test,
    #[cfg(not(test))]
    Wayland(u64),
    #[cfg(not(test))]
    X11(u32),
}
pub(super) struct Copy {
    pub layers: compositor::layer_clipboard::Layers,
    pub pixels: Arc<image::RgbaImage>,
    pub owner: Owner,
}
impl Editor {
    pub(super) fn clipboard_owner(&self) -> Result<Owner> {
        #[cfg(test)]
        {
            Ok(Owner::Test)
        }
        #[cfg(not(test))]
        {
            if let Some(clipboard) = &self.wayland_clipboard {
                Ok(Owner::Wayland(clipboard.revision()))
            } else {
                compositor::native_clipboard::x11_owner().map(Owner::X11)
            }
        }
    }
    pub(super) fn can_copy_layers(&self) -> bool {
        self.can_edit_layers()
            && !self.tools.mask_target
            && self
                .current_document()
                .is_some_and(|d| d.selection.is_none() && d.active.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};
    #[test]
    fn layer_clipboard_preserves_folders_across_projects_and_external_pixels_stay_pixels() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
        let doc = &mut editor.session_mut().document;
        let first = doc.active.unwrap();
        doc.add(compositor::document::Layer::blank("Second", 4, 4))
            .unwrap();
        doc.selected.insert(first);
        compositor::layer_ops::group(doc).unwrap();
        let original = doc.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Layer clipboard").size(1280., 850.),
                editor,
            )
            .unwrap();
        cx.update(view, |e, cx| {
            assert!(e.clipboard_available(Action::Copy));
            e.clipboard_action(Action::Copy, cx).unwrap();
            e.action(Action::Paste, cx);
            assert_eq!(e.session().document.layers.len(), 6);
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
            e.add_empty_tab();
            e.tabs[e.current].set_document(Document::new(8, 8).unwrap(), None);
            e.action(Action::Paste, cx);
            assert_eq!(e.session().document.layers.len(), 4);
            assert!(e.session().document.active_layer().unwrap().is_group());
            let external = image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 30, 40, 255]));
            e.paste_pixels(e.tabs[e.current].id, external).unwrap();
            assert_eq!(e.session().document.layers.len(), 5);
            assert!(!e.session().document.active_layer().unwrap().is_group());
            e.session_mut().document.selection = Some(compositor::selection::Selection::rectangle(
                8,
                8,
                [0., 0.],
                [8., 8.],
                false,
            ));
            e.clipboard_action(Action::Copy, cx).unwrap();
            assert!(e.layer_clipboard.is_none());
        })
        .unwrap();
    }
}
