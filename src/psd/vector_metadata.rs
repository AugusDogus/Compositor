//! Complete descriptor fields missing from ag-psd 0.3.0, without reimplementing
//! Photoshop's binary descriptor parser. Entries follow physical PSD layer order.
use crate::{Result, invalid};
use ag_psd::{
    descriptor::{Descriptor, DescriptorValue as Value},
    psd as ps,
};

#[derive(Default)]
pub(super) struct Metadata {
    pub unsupported_stroke_blend: bool,
    pub mask: Option<ps::LayerVectorMask>,
    pub origin: Option<ps::VectorOrigination>,
    pub stroke: Option<Descriptor>,
}
impl Metadata {
    pub fn read(&mut self, key: &[u8], payload: &[u8]) -> Result<()> {
        if matches!(key, b"vmsk" | b"vsms") {
            let mut reader = ag_psd::reader::PsdReader::new(payload, None, None);
            let mut info = ps::LayerAdditionalInfo::default();
            let options = ps::ReadOptions::default();
            let mut context = ag_psd::additional_info::ReadCtx {
                options: &options,
                large: false,
            };
            ag_psd::additional_info::vector_keys::read(
                "vmsk",
                &mut reader,
                &mut info,
                &|r| payload.len().saturating_sub(r.offset),
                &mut context,
            )
            .map_err(|e| invalid(format!("PSD vector mask is invalid: {e}")))?;
            self.mask = info.vector_mask;
            return Ok(());
        }
        if !matches!(key, b"vogk" | b"vstk") {
            return Ok(());
        }
        let payload = if key == b"vogk" {
            payload
                .get(4..)
                .ok_or_else(|| invalid("PSD vector origination is truncated."))?
        } else {
            payload
        };
        let mut reader = ag_psd::reader::PsdReader::new(payload, None, None);
        let descriptor = ag_psd::descriptor::read_version_and_descriptor(&mut reader)
            .map_err(|e| invalid(format!("PSD vector descriptor is invalid: {e}")))?;
        if key == b"vstk" {
            self.unsupported_stroke_blend = enum_value(descriptor.get("strokeStyleBlendMode"))
                .is_some_and(|mode| !matches!(mode, "Nrml" | "normal"));
            self.stroke = Some(descriptor);
            return Ok(());
        }
        let Some(Value::List(items)) = descriptor.get("keyDescriptorList") else {
            return Ok(());
        };
        let mut list = Vec::new();
        for item in items {
            let Value::Descriptor(item) = item else {
                return Err(invalid("PSD vector origin entry is not a descriptor."));
            };
            let mut origin = ps::KeyDescriptorItem {
                key_origin_type: number(item.get("keyOriginType")),
                key_shape_invalidated: boolean(item.get("keyShapeInvalidated")),
                ..Default::default()
            };
            if let Some(Value::Descriptor(bounds)) = item.get("keyOriginShapeBBox") {
                origin.key_origin_shape_bounding_box = Some(ps::UnitsBounds {
                    left: unit(bounds.get("Left"))?,
                    top: unit(bounds.get("Top "))?,
                    right: unit(bounds.get("Rght"))?,
                    bottom: unit(bounds.get("Btom"))?,
                });
            }
            if let Some(Value::Descriptor(r)) = item.get("keyOriginRRectRadii") {
                origin.key_origin_r_rect_radii = Some(ps::RRectRadii {
                    top_left: unit(r.get("topLeft"))?,
                    top_right: unit(r.get("topRight"))?,
                    bottom_left: unit(r.get("bottomLeft"))?,
                    bottom_right: unit(r.get("bottomRight"))?,
                });
            }
            if let Some(Value::Descriptor(t)) = item.get("Trnf") {
                origin.transform = Some(
                    ["xx", "xy", "yx", "yy", "tx", "ty"]
                        .iter()
                        .map(|key| {
                            number(t.get(key)).ok_or_else(|| {
                                invalid("PSD vector transform contains invalid values.")
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                );
            }
            list.push(origin);
        }
        self.origin = Some(ps::VectorOrigination {
            key_descriptor_list: list,
        });
        Ok(())
    }
    fn apply(self, layer: &mut ps::Layer) -> Result<()> {
        if self.mask.is_some() {
            layer.additional_info.vector_mask = self.mask;
        }
        if self.origin.is_some() {
            layer.additional_info.vector_origination = self.origin;
        }
        if let Some(desc) = self.stroke {
            let stroke = layer
                .additional_info
                .vector_stroke
                .get_or_insert_with(Default::default);
            if desc.get("strokeStyleLineWidth").is_some() {
                stroke.line_width = Some(unit(desc.get("strokeStyleLineWidth"))?);
            }
            stroke.opacity = number(desc.get("strokeStyleOpacity")).map(|v| v / 100.);
            stroke.line_cap_type = match enum_value(desc.get("strokeStyleLineCapType")) {
                Some("strokeStyleRoundCap") => Some(ps::LineCapType::Round),
                Some("strokeStyleSquareCap") => Some(ps::LineCapType::Square),
                _ => Some(ps::LineCapType::Butt),
            };
            stroke.line_join_type = match enum_value(desc.get("strokeStyleLineJoinType")) {
                Some("strokeStyleRoundJoin") => Some(ps::LineJoinType::Round),
                Some("strokeStyleBevelJoin") => Some(ps::LineJoinType::Bevel),
                _ => Some(ps::LineJoinType::Miter),
            };
            stroke.line_alignment = match enum_value(desc.get("strokeStyleLineAlignment")) {
                Some("strokeStyleAlignInside") => Some(ps::LineAlignment::Inside),
                Some("strokeStyleAlignOutside") => Some(ps::LineAlignment::Outside),
                _ => Some(ps::LineAlignment::Center),
            };
            if let Some(Value::List(values)) = desc.get("strokeStyleLineDashSet") {
                stroke.line_dash_set = Some(
                    values
                        .iter()
                        .map(|v| unit(Some(v)))
                        .collect::<Result<Vec<_>>>()?,
                );
            }
        }
        Ok(())
    }
}
fn number(value: Option<&Value>) -> Option<f64> {
    let number = match value? {
        Value::Integer(v) => f64::from(*v),
        Value::Double(v) => *v,
        Value::UnitDouble(v) => v.value,
        _ => return None,
    };
    number.is_finite().then_some(number)
}
fn unit(value: Option<&Value>) -> Result<ps::UnitsValue> {
    let n = number(value).ok_or_else(|| invalid("PSD vector size is missing or invalid."))?;
    let units = match value {
        Some(Value::UnitDouble(v)) => match v.units.as_str() {
            "#Pxl" | "Pixels" => ps::Units::Pixels,
            "#Pnt" | "Points" => ps::Units::Points,
            _ => return Err(invalid("PSD vector uses unsupported size units.")),
        },
        _ => ps::Units::Pixels,
    };
    Ok(ps::UnitsValue { units, value: n })
}
fn boolean(value: Option<&Value>) -> Option<bool> {
    if let Some(Value::Boolean(v)) = value {
        Some(*v)
    } else {
        None
    }
}
fn enum_value(value: Option<&Value>) -> Option<&str> {
    if let Some(Value::Enum(v)) = value {
        v.rsplit('.').next()
    } else {
        None
    }
}

pub(super) fn apply(psd: &mut ps::Psd, metadata: Vec<Metadata>) -> Result<()> {
    fn visit(
        layers: &mut [ps::Layer],
        metadata: &mut std::vec::IntoIter<Metadata>,
        size: [f64; 2],
    ) -> Result<()> {
        for layer in layers {
            if let Some(children) = &mut layer.children {
                visit(children, metadata, size)?;
            }
            let entry = metadata.next().ok_or_else(|| {
                invalid("PSD vector metadata does not match its layer hierarchy.")
            })?;
            entry.apply(layer)?;
            // ag-psd currently reads 8.24 path coordinates at unit canvas size.
            if let Some(mask) = &mut layer.additional_info.vector_mask {
                for path in &mut mask.paths {
                    for knot in &mut path.knots {
                        for (i, value) in knot.points.iter_mut().enumerate() {
                            *value *= size[i % 2];
                        }
                    }
                }
            }
        }
        Ok(())
    }
    let mut entries = metadata.into_iter();
    visit(
        psd.children.as_deref_mut().unwrap_or_default(),
        &mut entries,
        [psd.width, psd.height],
    )?;
    if entries.next().is_some() {
        return Err(invalid(
            "PSD vector metadata contains unmatched layer records.",
        ));
    }
    Ok(())
}
