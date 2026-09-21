//! EditorSession names transforms by their final operation and actual target.
use super::*;

impl Editor {
    pub(in crate::ui) fn transform_history_label(&self, perspective: bool) -> &'static str {
        let doc = self.session().committed_document();
        let group = doc.selected.len() > 1 || doc.active_layer().is_some_and(|l| l.is_group());
        let mask = !group
            && self.tools.mask_target
            && doc
                .active_layer()
                .and_then(|l| l.mask.as_ref())
                .is_some_and(|m| !m.linked);
        match (perspective, group, mask) {
            (true, true, _) => "Distort Layers",
            (true, false, true) => "Distort Layer Mask",
            (true, false, false) => "Distort",
            (false, true, _) => "Transform Layers",
            (false, false, true) => "Transform Layer Mask",
            (false, false, false) => "Transform Layer",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::menus::history_tests::history;
    use crate::ui::transform_header::layer_tests::pointer;
    use quickgui::PointerPhase;

    #[derive(Clone, Copy)]
    enum Target {
        Pixels,
        LinkedMask,
        Mask,
        Layers,
        Folder,
    }

    const TARGETS: [(Target, &str, &str); 5] = [
        (Target::Pixels, "Transform Layer", "Distort"),
        (Target::LinkedMask, "Transform Layer", "Distort"),
        (Target::Mask, "Transform Layer Mask", "Distort Layer Mask"),
        (Target::Layers, "Transform Layers", "Distort Layers"),
        (Target::Folder, "Transform Layers", "Distort Layers"),
    ];

    fn editor(target: Target) -> Editor {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(100, 80).unwrap();
        compositor::edits::fill(&mut doc, [80, 140, 200, 255], false, false).unwrap();
        match target {
            Target::Pixels => {}
            Target::LinkedMask | Target::Mask => {
                compositor::edits::add_mask(&mut doc, false).unwrap();
                doc.layers[0].mask.as_mut().unwrap().linked = matches!(target, Target::LinkedMask);
                e.tools.mask_target = true;
            }
            Target::Layers => {
                compositor::layer_ops::duplicate_active(&mut doc).unwrap();
                doc.selected = doc.layers.iter().map(|l| l.id).collect();
            }
            Target::Folder => compositor::layer_ops::group(&mut doc).unwrap(),
        }
        e.tabs = vec![Session::new(doc, None).into()];
        e.tools.tool = Tool::Move;
        e
    }

    #[test]
    fn affine_history_names_the_target_for_fields_nudges_moves_and_resize_handles() {
        for (target, label, _) in TARGETS {
            for operation in ["fields", "nudge", "move", "resize"] {
                let mut e = editor(target);
                let before = e.session().document.clone();
                match operation {
                    "fields" => {
                        e.header_transform_input(0, "5").unwrap();
                        e.finish_toolbar_transform(true).unwrap();
                    }
                    "nudge" => e.nudge(&Key::ArrowRight, Modifiers::empty()).unwrap(),
                    "move" => {
                        e.tools.show_transform_controls = false;
                        pointer(&mut e, PointerPhase::Down, [40., 40.], Modifiers::empty());
                        pointer(&mut e, PointerPhase::Up, [55., 48.], Modifiers::empty());
                    }
                    _ => {
                        e.tools.show_transform_controls = true;
                        pointer(&mut e, PointerPhase::Down, [100., 80.], Modifiers::empty());
                        pointer(&mut e, PointerPhase::Up, [120., 96.], Modifiers::empty());
                    }
                }
                history(&mut e, label, before);
            }
        }
    }

    #[test]
    fn distortion_history_uses_the_final_operation_and_target() {
        for (target, _, label) in TARGETS {
            let mut e = editor(target);
            let before = e.session().document.clone();
            e.header_transform_input(0, "5").unwrap();
            pointer(&mut e, PointerPhase::Down, [5., 0.], Modifiers::CONTROL);
            pointer(&mut e, PointerPhase::Up, [15., 10.], Modifiers::CONTROL);
            e.finish_toolbar_transform(true).unwrap();
            history(&mut e, label, before);
        }
    }

    #[test]
    fn cancelling_a_distortion_drag_keeps_the_affine_history_name() {
        let mut e = editor(Target::Pixels);
        let before = e.session().document.clone();
        e.header_transform_input(0, "5").unwrap();
        let affine = e.session().document.clone();
        pointer(&mut e, PointerPhase::Down, [5., 0.], Modifiers::CONTROL);
        pointer(&mut e, PointerPhase::Move, [15., 10.], Modifiers::CONTROL);
        assert_ne!(e.session().document, affine);
        pointer(&mut e, PointerPhase::Cancel, [15., 10.], Modifiers::CONTROL);
        assert_eq!(e.session().document, affine);
        e.finish_toolbar_transform(true).unwrap();
        history(&mut e, "Transform Layer", before);
    }
}
