//! Adjustment-layer creation is committed before its settings panel opens.
use super::*;
use compositor::adjustment::{Adjustment, Color as AdjustmentColor, GradientMap, Grain};
use compositor::document::{Layer, LayerContent};

pub(super) fn title(kind: Kind) -> &'static str {
    match kind {
        Kind::HueSaturation => "Hue/Saturation",
        Kind::Levels => "Levels",
        Kind::Curves => "Curves",
        Kind::Exposure => "Exposure",
        Kind::GradientMap => "Gradient Map",
        Kind::Grain => "Grain",
        Kind::Invert => "Invert",
        Kind::BlackWhite => "Black & White",
        Kind::ColorBalance => "Color Balance",
    }
}

impl Editor {
    pub(super) fn add_adjustment_layer(&mut self, kind: Kind) -> Result<()> {
        let mut settings = Adjustment::new(kind);
        if kind == Kind::GradientMap {
            let color = |rgba: [u8; 4]| AdjustmentColor {
                red: f64::from(rgba[0]) / 255.,
                green: f64::from(rgba[1]) / 255.,
                blue: f64::from(rgba[2]) / 255.,
            };
            settings.gradient_map_settings = Some(GradientMap {
                shadows: color(self.tools.brush.color),
                highlights: color(self.tools.background),
                reversed: false,
            });
        } else if kind == Kind::Grain {
            settings.grain_settings = Some(Grain {
                seed: uuid::Uuid::new_v4().as_u128() as u32,
                ..Grain::default()
            });
        }
        self.session_mut()
            .edit(&format!("New {} Adjustment", title(kind)), |doc| {
                let mut layer = Layer::blank(title(kind), doc.width, doc.height);
                layer.parent = doc.active_layer().and_then(|active| {
                    if active.is_group() {
                        Some(active.id)
                    } else {
                        active.parent
                    }
                });
                layer.content = LayerContent::Adjustment(Box::new(settings));
                let index = doc
                    .layers
                    .iter()
                    .position(|l| Some(l.id) == doc.active)
                    .map_or(doc.layers.len(), |i| i + 1);
                doc.add(layer)?;
                if let Some(layer) = doc.layers.pop() {
                    doc.layers.insert(index, layer);
                }
                Ok(())
            })?;
        self.tools.mask_target = false;
        if let Some(parent) = self
            .session()
            .document
            .active_layer()
            .and_then(|layer| layer.parent)
        {
            self.session_mut().collapsed.remove(&parent);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_color_adjustments_are_editable_and_invert_has_no_empty_dialog() {
        let mut e = Editor::with_test_document();
        e.open_adjustment(Some(Kind::Invert)).unwrap();
        assert!(e.adjustment_edit.is_none());
        assert!(
            matches!(&e.session().document.active_layer().unwrap().content,LayerContent::Adjustment(a) if a.kind==Kind::Invert)
        );
        for kind in [Kind::BlackWhite, Kind::ColorBalance] {
            e.open_adjustment(Some(kind)).unwrap();
            let fields = super::super::adjustment_fields::fields(
                &e.adjustment_edit.as_ref().unwrap().settings,
            );
            let parsed = super::super::adjustment_fields::parse(
                &e.adjustment_edit.as_ref().unwrap().settings,
                &fields.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(parsed.kind, kind);
            e.cancel_adjustment();
        }
    }
    #[test]
    fn new_adjustments_use_source_names_parent_palette_and_independent_creation_history() {
        let mut e = Editor::with_test_document();
        let doc = &mut e.session_mut().document;
        let mut group = Layer::blank("Group", doc.width, doc.height);
        group.content = LayerContent::Group;
        let group_id = group.id;
        doc.add(group).unwrap();
        e.session_mut().collapsed.insert(group_id);
        e.tools.brush.color = [128, 64, 32, 255];
        e.tools.background = [16, 32, 48, 255];
        let original = e.session().document.clone();
        e.open_adjustment(Some(Kind::GradientMap)).unwrap();
        let created = e.session().document.clone();
        let layer = created.active_layer().unwrap();
        assert_eq!(layer.name, "Gradient Map");
        assert_eq!(layer.parent, Some(group_id));
        assert!(!e.session().collapsed.contains(&group_id));
        let LayerContent::Adjustment(settings) = &layer.content else {
            panic!("Missing adjustment");
        };
        let map = settings.gradient_map_settings.unwrap();
        assert_eq!(map.shadows.rgb(), [128. / 255., 64. / 255., 32. / 255.]);
        assert_eq!(map.highlights.rgb(), [16. / 255., 32. / 255., 48. / 255.]);
        e.cancel_adjustment();
        assert_eq!(e.session().document, created);
        assert_eq!(
            e.session().undo_label(),
            Some("New Gradient Map Adjustment")
        );
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
        e.session_mut().redo();
        assert_eq!(e.session().document, created);
    }

    #[test]
    fn preview_off_restores_new_adjustment_defaults_and_apply_has_its_own_undo_step() {
        let mut e = Editor::with_test_document();
        let original = e.session().document.clone();
        e.open_adjustment(Some(Kind::Exposure)).unwrap();
        let created = e.session().document.clone();
        if let Some(Form::Edit { fields, .. }) = &mut e.modal {
            fields[0].1 = "1".into();
        }
        e.preview_adjustment().unwrap();
        assert_ne!(e.session().document, created);
        e.adjustment_edit.as_mut().unwrap().preview = false;
        e.preview_adjustment().unwrap();
        assert_eq!(e.session().document, created);
        assert!(e.session().document.active_layer().unwrap().visible);
        e.finish_adjustment().unwrap();
        assert_eq!(e.session().undo_label(), Some("Edit Exposure Adjustment"));
        e.session_mut().undo();
        assert_eq!(e.session().document, created);
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
    }
}
