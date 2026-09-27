//! ABR descriptors use Photoshop's "Object Descriptor" binary format.
//!
//! Descriptor structure (big-endian throughout):
//!   - 4 bytes: class name length (in chars, Unicode)
//!   - N*2 bytes: class name (UTF-16BE)
//!   - 4 bytes: class ID (length-prefixed string or 4-char code)
//!   - 4 bytes: number of descriptor items
//!   - For each item:
//!     - key (length-prefixed or 4-char)
//!     - 4 bytes: type tag (e.g. "TEXT", "long", "bool", "Objc", etc.)
//!     - value (type-dependent)
//!
//! The desc block header has format_version (u32 = 16), then either:
//!   - Photoshop native: descriptor starts immediately (class_name_length >= 1)
//!   - Alternative: count (u32 = 1), then descriptor (class_name_length = 0)
//!
//! Reference: Adobe Photoshop File Formats Specification (Action Descriptor)

use byteorder::{BigEndian, ReadBytesExt};
use std::io::{self, Cursor, Read};

const MAX_DESCRIPTOR_STRING_LEN: usize = 1024 * 1024;
pub(crate) const MAX_DESCRIPTOR_DEPTH: usize = 64;

#[derive(Debug, Clone)]
pub struct BrushDescInfo {
    pub name: String,
    pub sampled_data_uuid: Option<String>,
    /// The `dualBrush > Brsh > sampledData` uuid — the samp tip used as this
    /// preset's dual-brush component. `Some` only when the sub-descriptor's
    /// `useDualBrush` flag is true; `None` otherwise. Used to attribute dropped
    /// dual-only tips back to the presets that reference them.
    pub dual_brush_uuid: Option<String>,
    /// True when the preset declares a `Shp ` dynamic-tip descriptor
    /// (bristle/erodible/airbrush tip). The parser lifts nothing else from it —
    /// presence only, so a preset that yields no output can say why.
    pub has_shape_tip: bool,
    /// The `Shp `-carrying inner `Brsh` class id read as a tip family
    /// (`dBrush` → bristle, `dTips` → erodible/airbrush); `None` when there is
    /// no `Shp ` or the class id is unrecognised.
    pub shape_tip_family: Option<crate::ShapeTipFamily>,
    /// The `Shp ` integer read against its class id as a tip shape; `None`
    /// when there is no `Shp `, its tag is not `long`, or
    /// the `(class id, index)` pair is outside the measured table.
    pub tip_shape: Option<crate::TipShape>,
    /// Per-brush dynamics read from the preset's descriptor.
    pub descriptor: crate::BrushDescriptor,
}

#[cfg(test)]
pub fn extract_all_brush_info(data: &[u8]) -> Vec<BrushDescInfo> {
    extract_all_brush_info_inner(data).unwrap_or_default()
}

#[cfg(test)]
pub fn extract_brush_name(data: &[u8]) -> Option<String> {
    let infos = extract_all_brush_info(data);
    infos.into_iter().next().map(|i| i.name)
}

pub(crate) fn extract_all_brush_info_inner(data: &[u8]) -> io::Result<Vec<BrushDescInfo>> {
    let mut cursor = Cursor::new(data);

    let _fmt_ver = cursor.read_u32::<BigEndian>()?;

    let peek_pos = cursor.position();
    let first_u32 = cursor.read_u32::<BigEndian>()?;

    if first_u32 == 0 {
        let next_u32 = cursor.read_u32::<BigEndian>()?;
        if next_u32 == 0 {
            let mut ci = [0u8; 4];
            cursor.read_exact(&mut ci)?;
        } else {
            let _class_id = read_id_from(&mut cursor, next_u32)?;
        }
    } else if first_u32 == 1 {
        let char_val = cursor.read_u16::<BigEndian>()?;
        if char_val == 0 {
            let _class_id = read_id(&mut cursor)?;
        } else {
            cursor.set_position(peek_pos + 4);
            let _class_name = read_unicode_string_from(&mut cursor, 0)?;
            let _class_id = read_id(&mut cursor)?;
        }
    } else {
        let _class_name = read_unicode_string_from(&mut cursor, first_u32 as usize)?;
        let _class_id = read_id(&mut cursor)?;
    }

    let item_count = cursor.read_u32::<BigEndian>()?;

    let mut results = Vec::new();

    for _ in 0..item_count {
        let key = read_id(&mut cursor)?;
        let type_tag = read_type_tag(&mut cursor)?;

        if key == "Brsh" && type_tag == "VlLs" {
            let vl_count = cursor.read_u32::<BigEndian>()?;
            for _ in 0..vl_count {
                let elem_tag = read_type_tag(&mut cursor)?;
                if elem_tag == "Objc" {
                    match parse_brush_preset_objc(&mut cursor) {
                        Ok(info) => results.push(info),
                        Err(_) => break,
                    }
                } else {
                    if skip_descriptor_value(&mut cursor, &elem_tag, 0).is_err() {
                        break;
                    }
                }
            }
        } else if skip_descriptor_value(&mut cursor, &type_tag, 0).is_err() {
            break;
        }
    }

    Ok(results)
}

fn parse_brush_preset_objc(cursor: &mut Cursor<&[u8]>) -> io::Result<BrushDescInfo> {
    let _class_name = read_unicode_string(cursor)?;
    let _class_id = read_id(cursor)?;

    let item_count = cursor.read_u32::<BigEndian>()?;

    let mut name = String::new();
    let mut uuid = None;
    let mut dual_brush_uuid = None;
    let mut has_shape_tip = false;
    let mut shape_tip_family = None;
    let mut tip_shape = None;
    let mut use_dual_brush = false;
    let mut dual: Option<crate::DualBrush> = None;
    let mut wet_edges = false;
    let mut noise = false;
    let mut buildup = false;
    let mut use_color_dynamics = false;
    let mut use_brush_pose = false;
    let mut brush_projection = false;
    let mut spacing_pct = None;
    let mut computed = None;
    let mut diameter_px = None;
    let mut sampled_angle_deg = None;
    let mut sampled_roundness_pct = None;
    let mut sampled_flip_x = None;
    let mut sampled_flip_y = None;
    let mut use_tip_dynamics = false;
    let mut use_scatter = false;
    let mut use_paint_dynamics = false;
    let mut sz_jitter = None;
    let mut sz_control = None;
    let mut op_control = None;
    let mut op_jitter = None;
    let mut flow_jitter = None;
    let mut wetness_jitter = None;
    let mut mix_jitter = None;
    let mut angle_control = None;
    let mut angle_jitter = None;
    let mut scatter_jitter = None;
    let mut scatter_count = None;
    let mut scatter_both_axes = None;
    let mut count_jitter = None;
    let mut minimum_diameter = None;
    let mut roundness_jitter = None;
    let mut minimum_roundness = None;
    let mut flip_x = None;
    let mut flip_y = None;
    let mut use_texture = false;
    let mut texture_pattern_id = None;
    let mut texture_pattern_name = None;
    let mut texture_depth = None;
    let mut texture_minimum_depth = None;
    let mut texture_scale = None;
    let mut texture_invert = None;
    let mut texture_each_tip = None;
    let mut texture_blend_mode = None;
    let mut texture_depth_jitter = None;
    let mut texture_brightness = None;
    let mut texture_contrast = None;
    let mut color_hue = None;
    let mut color_saturation = None;
    let mut color_brightness = None;
    let mut color_purity = None;
    let mut color_fg_bg_jitter = None;
    let mut color_per_tip: Option<bool> = None;

    for _ in 0..item_count {
        let key = read_id(cursor)?;
        let type_tag = read_type_tag(cursor)?;

        if key == "Nm  " && type_tag == "TEXT" {
            name = read_unicode_string(cursor)?;
        } else if key == "sampledData" && type_tag == "TEXT" {
            uuid = Some(read_unicode_string(cursor)?);
        } else if key == "Brsh" && type_tag == "Objc" {
            let inner = parse_inner_brush(cursor)?;
            if uuid.is_none() {
                uuid = inner.uuid;
            }
            if spacing_pct.is_none() {
                spacing_pct = inner.spacing_pct;
            }
            if computed.is_none() {
                computed = inner.computed;
            }
            if diameter_px.is_none() {
                diameter_px = inner.diameter_px;
            }
            if sampled_angle_deg.is_none() {
                sampled_angle_deg = inner.sampled_angle_deg;
            }
            if sampled_roundness_pct.is_none() {
                sampled_roundness_pct = inner.sampled_roundness_pct;
            }
            if sampled_flip_x.is_none() {
                sampled_flip_x = inner.sampled_flip_x;
            }
            if sampled_flip_y.is_none() {
                sampled_flip_y = inner.sampled_flip_y;
            }
            has_shape_tip |= inner.has_shape_tip;
            shape_tip_family = shape_tip_family.or(inner.shape_tip_family);
            tip_shape = tip_shape.or(inner.tip_shape);
        } else if key == "dualBrush" && type_tag == "Objc" {
            let (flag, section) = parse_dual_brush(cursor)?;
            use_dual_brush = flag;
            dual_brush_uuid = section.uuid.clone();
            dual = flag.then_some(section);
        } else if key == "useTipDynamics" && type_tag == "bool" {
            use_tip_dynamics = cursor.read_u8()? != 0;
        } else if key == "useScatter" && type_tag == "bool" {
            use_scatter = cursor.read_u8()? != 0;
        } else if key == "usePaintDynamics" && type_tag == "bool" {
            use_paint_dynamics = cursor.read_u8()? != 0;
        } else if key == "szVr" && type_tag == "Objc" {
            let b = parse_brvr(cursor)?;
            sz_jitter = b.jitter_pct;
            sz_control = b.control;
        } else if key == "opVr" && type_tag == "Objc" {
            let b = parse_brvr(cursor)?;
            op_control = b.control;
            op_jitter = b.jitter_pct;
        } else if key == "prVr" && type_tag == "Objc" {
            flow_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "wtVr" && type_tag == "Objc" {
            wetness_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "mxVr" && type_tag == "Objc" {
            mix_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "angleDynamics" && type_tag == "Objc" {
            let b = parse_brvr(cursor)?;
            angle_control = b.control;
            angle_jitter = b.jitter_pct;
        } else if key == "scatterDynamics" && type_tag == "Objc" {
            scatter_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "countDynamics" && type_tag == "Objc" {
            count_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "Cnt " && type_tag == "doub" {
            scatter_count = Some(cursor.read_f64::<BigEndian>()?);
        } else if key == "bothAxes" && type_tag == "bool" {
            scatter_both_axes = Some(cursor.read_u8()? != 0);
        } else if key == "minimumDiameter" && type_tag == "UntF" {
            minimum_diameter = Some(read_unit_float(cursor)?);
        } else if key == "roundnessDynamics" && type_tag == "Objc" {
            roundness_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "minimumRoundness" && type_tag == "UntF" {
            minimum_roundness = Some(read_unit_float(cursor)?);
        } else if key == "flipX" && type_tag == "bool" {
            flip_x = Some(cursor.read_u8()? != 0);
        } else if key == "flipY" && type_tag == "bool" {
            flip_y = Some(cursor.read_u8()? != 0);
        } else if key == "useTexture" && type_tag == "bool" {
            use_texture = cursor.read_u8()? != 0;
        } else if key == "useColorDynamics" && type_tag == "bool" {
            use_color_dynamics = cursor.read_u8()? != 0;
        } else if key == "H   " && type_tag == "UntF" {
            color_hue = Some(read_unit_float(cursor)?);
        } else if key == "Strt" && type_tag == "UntF" {
            color_saturation = Some(read_unit_float(cursor)?);
        } else if key == "Brgh" && type_tag == "UntF" {
            color_brightness = Some(read_unit_float(cursor)?);
        } else if key == "purity" && type_tag == "UntF" {
            color_purity = Some(read_unit_float(cursor)?);
        } else if key == "clVr" && type_tag == "Objc" {
            color_fg_bg_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "colorDynamicsPerTip" && type_tag == "bool" {
            color_per_tip = Some(cursor.read_u8()? != 0);
        } else if key == "useBrushPose" && type_tag == "bool" {
            use_brush_pose = cursor.read_u8()? != 0;
        } else if key == "brushProjection" && type_tag == "bool" {
            brush_projection = cursor.read_u8()? != 0;
        } else if key == "Wtdg" && type_tag == "bool" {
            wet_edges = cursor.read_u8()? != 0;
        } else if key == "Nose" && type_tag == "bool" {
            noise = cursor.read_u8()? != 0;
        } else if key == "Rpt " && type_tag == "bool" {
            buildup = cursor.read_u8()? != 0;
        } else if key == "textureBlendMode" && type_tag == "enum" {
            let _enum_type = read_id(cursor)?;
            texture_blend_mode = Some(read_id(cursor)?);
        } else if key == "textureDepth" && type_tag == "UntF" {
            texture_depth = Some(read_unit_float(cursor)?);
        } else if key == "minimumDepth" && type_tag == "UntF" {
            texture_minimum_depth = Some(read_unit_float(cursor)?);
        } else if key == "textureDepthDynamics" && type_tag == "Objc" {
            texture_depth_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "Txtr" && type_tag == "Objc" {
            let t = parse_txtr(cursor)?;
            texture_pattern_name = t.name;
            texture_pattern_id = t.id;
        } else if key == "textureScale" && type_tag == "UntF" {
            texture_scale = Some(read_unit_float(cursor)?);
        } else if key == "InvT" && type_tag == "bool" {
            texture_invert = Some(cursor.read_u8()? != 0);
        } else if key == "TxtC" && type_tag == "bool" {
            texture_each_tip = Some(cursor.read_u8()? != 0);
        } else if key == "textureBrightness" && type_tag == "long" {
            texture_brightness = Some(cursor.read_i32::<BigEndian>()? as i64);
        } else if key == "textureContrast" && type_tag == "long" {
            texture_contrast = Some(cursor.read_i32::<BigEndian>()? as i64);
        } else if skip_descriptor_value(cursor, &type_tag, 0).is_err() {
            break;
        }
    }

    Ok(BrushDescInfo {
        name,
        sampled_data_uuid: uuid,
        dual_brush_uuid,
        has_shape_tip,
        shape_tip_family,
        tip_shape,
        descriptor: crate::BrushDescriptor {
            spacing_pct,
            computed,
            diameter_px,
            scatter_amount_pct: if use_scatter { scatter_jitter } else { None },
            size_jitter_pct: if use_tip_dynamics { sz_jitter } else { None },
            angle_control: if use_tip_dynamics {
                angle_control
            } else {
                None
            },
            angle_jitter_pct: if use_tip_dynamics { angle_jitter } else { None },
            pressure_size_control: if use_tip_dynamics { sz_control } else { None },
            pressure_opacity_control: if use_paint_dynamics { op_control } else { None },
            opacity_jitter_pct: if use_paint_dynamics { op_jitter } else { None },
            flow_jitter_pct: if use_paint_dynamics {
                flow_jitter
            } else {
                None
            },
            wetness_jitter_pct: if use_paint_dynamics {
                wetness_jitter
            } else {
                None
            },
            mix_jitter_pct: if use_paint_dynamics { mix_jitter } else { None },
            shape_angle_deg: sampled_angle_deg,
            shape_roundness_pct: sampled_roundness_pct,
            scatter_count: if use_scatter { scatter_count } else { None },
            scatter_both_axes: if use_scatter { scatter_both_axes } else { None },
            count_jitter_pct: if use_scatter { count_jitter } else { None },
            minimum_diameter_pct: if use_tip_dynamics {
                minimum_diameter
            } else {
                None
            },
            roundness_jitter_pct: if use_tip_dynamics {
                roundness_jitter
            } else {
                None
            },
            minimum_roundness_pct: if use_tip_dynamics {
                minimum_roundness
            } else {
                None
            },
            flip_x_jitter: if use_tip_dynamics { flip_x } else { None },
            flip_y_jitter: if use_tip_dynamics { flip_y } else { None },
            tip_flip_x: sampled_flip_x,
            tip_flip_y: sampled_flip_y,
            texture_pattern_id: if use_texture {
                texture_pattern_id
            } else {
                None
            },
            texture_pattern_name: if use_texture {
                texture_pattern_name
            } else {
                None
            },
            texture_depth_pct: if use_texture { texture_depth } else { None },
            texture_minimum_depth_pct: if use_texture {
                texture_minimum_depth
            } else {
                None
            },
            texture_scale_pct: if use_texture { texture_scale } else { None },
            texture_invert: if use_texture { texture_invert } else { None },
            texture_each_tip: if use_texture { texture_each_tip } else { None },
            texture_blend_mode: if use_texture {
                texture_blend_mode
            } else {
                None
            },
            texture_depth_jitter_pct: if use_texture {
                texture_depth_jitter
            } else {
                None
            },
            texture_brightness: if use_texture {
                texture_brightness
            } else {
                None
            },
            texture_contrast: if use_texture { texture_contrast } else { None },
            use_dual_brush,
            dual,
            wet_edges,
            noise,
            buildup,
            use_color_dynamics,
            color_hue_jitter_pct: if use_color_dynamics { color_hue } else { None },
            color_saturation_jitter_pct: if use_color_dynamics {
                color_saturation
            } else {
                None
            },
            color_brightness_jitter_pct: if use_color_dynamics {
                color_brightness
            } else {
                None
            },
            color_purity_pct: if use_color_dynamics {
                color_purity
            } else {
                None
            },
            color_fg_bg_jitter_pct: if use_color_dynamics {
                color_fg_bg_jitter
            } else {
                None
            },
            color_dynamics_per_tip: if use_color_dynamics {
                color_per_tip
            } else {
                None
            },
            use_brush_pose,
            brush_projection,
        },
    })
}

fn parse_dual_brush(cursor: &mut Cursor<&[u8]>) -> io::Result<(bool, crate::DualBrush)> {
    let _class_name = read_unicode_string(cursor)?;
    let _class_id = read_id(cursor)?;
    let item_count = cursor.read_u32::<BigEndian>()?;

    let mut use_flag = false;
    let mut section = crate::DualBrush::default();
    let mut scatter_jitter = None;
    let mut scatter_count = None;
    let mut scatter_both_axes = None;
    let mut count_jitter = None;
    for _ in 0..item_count {
        let key = read_id(cursor)?;
        let type_tag = read_type_tag(cursor)?;
        if key == "useDualBrush" && type_tag == "bool" {
            use_flag = cursor.read_u8()? != 0;
        } else if key == "Brsh" && type_tag == "Objc" {
            let inner = parse_inner_brush(cursor)?;
            section.uuid = inner.uuid;
            section.spacing_pct = inner.spacing_pct;
            section.computed = inner.computed;
            section.diameter_px = inner.diameter_px;
            section.shape_angle_deg = inner.sampled_angle_deg;
            section.shape_roundness_pct = inner.sampled_roundness_pct;
            section.tip_flip_x = inner.sampled_flip_x;
            section.tip_flip_y = inner.sampled_flip_y;
        } else if key == "useScatter" && type_tag == "bool" {
            section.use_scatter = cursor.read_u8()? != 0;
        } else if key == "scatterDynamics" && type_tag == "Objc" {
            scatter_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "countDynamics" && type_tag == "Objc" {
            count_jitter = parse_brvr(cursor)?.jitter_pct;
        } else if key == "Cnt " && type_tag == "doub" {
            scatter_count = Some(cursor.read_f64::<BigEndian>()?);
        } else if key == "bothAxes" && type_tag == "bool" {
            scatter_both_axes = Some(cursor.read_u8()? != 0);
        } else if key == "Spcn" && type_tag == "UntF" {
            section.scatter_spacing_pct = Some(read_unit_float(cursor)?);
        } else if key == "BlnM" && type_tag == "enum" {
            let _enum_type = read_id(cursor)?;
            section.blend_mode = Some(read_id(cursor)?);
        } else if key == "Flip" && type_tag == "bool" {
            section.flip = Some(cursor.read_u8()? != 0);
        } else if skip_descriptor_value(cursor, &type_tag, 0).is_err() {
            break;
        }
    }
    if section.use_scatter {
        section.scatter_amount_pct = scatter_jitter;
        section.scatter_count = scatter_count;
        section.scatter_both_axes = scatter_both_axes;
        section.count_jitter_pct = count_jitter;
    }
    section.uuid = section.uuid.filter(|_| use_flag);
    Ok((use_flag, section))
}

struct InnerBrush {
    uuid: Option<String>,
    spacing_pct: Option<f64>,
    /// `Some` iff the inner class id is `computedBrush` — the witnessed
    /// procedural-tip geometry keys (Dmtr/Hrdn/Angl/Rndn), each optional.
    computed: Option<crate::ComputedGeometry>,
    /// `Dmtr` (`UntF #Pxl`) — tip diameter, read for BOTH sampled and computed
    /// inner classes (mirrors `computed.diameter_px` when computed).
    diameter_px: Option<f64>,
    /// `Brsh:sampledBrush/Angl` (`#Ang`) — sampled-tip rotation, read only when
    /// the inner class id is NOT `computedBrush`.
    sampled_angle_deg: Option<f64>,
    /// `Brsh:sampledBrush/Rndn` (`#Prc`) — sampled-tip roundness percent.
    sampled_roundness_pct: Option<f64>,
    /// `Brsh:sampledBrush/flipX` (`bool`) — static horizontal tip flip. Read only
    /// when the inner class id is NOT `computedBrush`.
    sampled_flip_x: Option<bool>,
    /// `Brsh:sampledBrush/flipY` (`bool`) — static vertical tip flip.
    sampled_flip_y: Option<bool>,
    /// `Brsh > Shp ` (`long`) — presence of the bristle/erodible/airbrush
    /// dynamic-tip descriptor.
    has_shape_tip: bool,
    /// The class id above read as a tip family, recorded only when `Shp ` is
    /// actually present (`dBrush` → bristle, `dTips` → erodible/airbrush).
    shape_tip_family: Option<crate::ShapeTipFamily>,
    /// The `Shp ` value resolved against that class id by [`tip_shape_for`];
    /// `None` unless the tag is `long` and the pair is in the measured table.
    tip_shape: Option<crate::TipShape>,
}

fn tip_shape_for(family: Option<crate::ShapeTipFamily>, index: i32) -> Option<crate::TipShape> {
    use crate::ShapeTipFamily as F;
    use crate::TipShape as S;
    Some(match (family?, index) {
        (F::Bristle, 0) => S::RoundPoint,
        (F::Bristle, 1) => S::RoundBlunt,
        (F::Bristle, 2) => S::RoundCurve,
        (F::Bristle, 3) => S::RoundAngle,
        (F::Bristle, 4) => S::RoundFan,
        (F::Bristle, 5) => S::FlatPoint,
        (F::Bristle, 6) => S::FlatBlunt,
        (F::Bristle, 7) => S::FlatCurve,
        (F::Bristle, 8) => S::FlatAngle,
        (F::Bristle, 9) => S::FlatFan,
        (F::Erodible, 0) => S::ErodiblePoint,
        (F::Erodible, 1) => S::ErodibleFlat,
        (F::Erodible, 2) => S::ErodibleRound,
        (F::Erodible, 3) => S::ErodibleSquare,
        (F::Erodible, 4) => S::ErodibleTriangle,
        (F::Erodible, 5) => S::AirbrushTip,
        _ => return None,
    })
}

fn parse_inner_brush(cursor: &mut Cursor<&[u8]>) -> io::Result<InnerBrush> {
    let _class_name = read_unicode_string(cursor)?;
    let class_id = read_id(cursor)?;
    let item_count = cursor.read_u32::<BigEndian>()?;

    let is_computed = class_id == "computedBrush";
    let class_family = match class_id.as_str() {
        "dBrush" => Some(crate::ShapeTipFamily::Bristle),
        "dTips" => Some(crate::ShapeTipFamily::Erodible),
        _ => None,
    };
    let mut uuid = None;
    let mut spacing_pct = None;
    let mut geometry = crate::ComputedGeometry::default();
    let mut diameter_px = None;
    let mut sampled_angle_deg = None;
    let mut sampled_roundness_pct = None;
    let mut sampled_flip_x = None;
    let mut sampled_flip_y = None;
    let mut has_shape_tip = false;
    let mut shape_tip_family = None;
    let mut tip_shape = None;
    for _ in 0..item_count {
        let key = read_id(cursor)?;
        let type_tag = read_type_tag(cursor)?;

        if key == "sampledData" && type_tag == "TEXT" {
            uuid = Some(read_unicode_string(cursor)?);
        } else if key == "Spcn" && type_tag == "UntF" {
            spacing_pct = Some(read_unit_float(cursor)?);
        } else if is_computed
            && type_tag == "UntF"
            && matches!(key.as_str(), "Dmtr" | "Hrdn" | "Angl" | "Rndn")
        {
            let value = read_unit_float(cursor)?;
            match key.as_str() {
                "Dmtr" => {
                    geometry.diameter_px = Some(value);
                    diameter_px = Some(value);
                }
                "Hrdn" => geometry.hardness_pct = Some(value),
                "Angl" => geometry.angle_deg = Some(value),
                "Rndn" => geometry.roundness_pct = Some(value),
                _ => unreachable!(),
            }
        } else if !is_computed
            && type_tag == "UntF"
            && matches!(key.as_str(), "Dmtr" | "Angl" | "Rndn")
        {
            let value = read_unit_float(cursor)?;
            match key.as_str() {
                "Dmtr" => diameter_px = Some(value),
                "Angl" => sampled_angle_deg = Some(value),
                "Rndn" => sampled_roundness_pct = Some(value),
                _ => unreachable!(),
            }
        } else if !is_computed && type_tag == "bool" && matches!(key.as_str(), "flipX" | "flipY") {
            let value = cursor.read_u8()? != 0;
            match key.as_str() {
                "flipX" => sampled_flip_x = Some(value),
                _ => sampled_flip_y = Some(value),
            }
        } else if key == "Shp " {
            has_shape_tip = true;
            shape_tip_family = class_family;
            if type_tag == "long" {
                let Ok(index) = cursor.read_i32::<BigEndian>() else {
                    break;
                };
                tip_shape = tip_shape_for(class_family, index);
            } else if skip_descriptor_value(cursor, &type_tag, 0).is_err() {
                break;
            }
        } else if skip_descriptor_value(cursor, &type_tag, 0).is_err() {
            break;
        }
    }

    Ok(InnerBrush {
        uuid,
        spacing_pct,
        computed: is_computed.then_some(geometry),
        diameter_px,
        sampled_angle_deg,
        sampled_roundness_pct,
        sampled_flip_x,
        sampled_flip_y,
        has_shape_tip,
        shape_tip_family,
        tip_shape,
    })
}

/// Parsed fields of a `brVr` dynamics block: the numeric control selector
/// (`bVTy`) and the jitter amount (`jitter`, a `#Prc` percent). Other `brVr`
/// keys (`fStp`, `Mnm`) are read past and ignored.
struct BrVr {
    control: Option<i32>,
    jitter_pct: Option<f64>,
}

fn parse_brvr(cursor: &mut Cursor<&[u8]>) -> io::Result<BrVr> {
    let _class_name = read_unicode_string(cursor)?;
    let _class_id = read_id(cursor)?;
    let item_count = cursor.read_u32::<BigEndian>()?;

    let mut control = None;
    let mut jitter_pct = None;
    for _ in 0..item_count {
        let key = read_id(cursor)?;
        let type_tag = read_type_tag(cursor)?;
        if key == "bVTy" && type_tag == "long" {
            control = Some(cursor.read_i32::<BigEndian>()?);
        } else if key == "jitter" && type_tag == "UntF" {
            jitter_pct = Some(read_unit_float(cursor)?);
        } else if skip_descriptor_value(cursor, &type_tag, 0).is_err() {
            break;
        }
    }
    Ok(BrVr {
        control,
        jitter_pct,
    })
}

/// Parsed contents of the `Txtr` object: the texture pattern name and UUID. Its
/// class id is `Ptrn` directly (no intermediate object).
struct Txtr {
    name: Option<String>,
    id: Option<String>,
}

fn parse_txtr(cursor: &mut Cursor<&[u8]>) -> io::Result<Txtr> {
    let _class_name = read_unicode_string(cursor)?;
    let _class_id = read_id(cursor)?;
    let item_count = cursor.read_u32::<BigEndian>()?;

    let mut name = None;
    let mut id = None;
    for _ in 0..item_count {
        let key = read_id(cursor)?;
        let type_tag = read_type_tag(cursor)?;
        if key == "Nm  " && type_tag == "TEXT" {
            name = Some(read_unicode_string(cursor)?);
        } else if key == "Idnt" && type_tag == "TEXT" {
            id = Some(read_unicode_string(cursor)?);
        } else if skip_descriptor_value(cursor, &type_tag, 0).is_err() {
            break;
        }
    }
    Ok(Txtr { name, id })
}

pub(crate) fn read_unicode_string(cursor: &mut Cursor<&[u8]>) -> io::Result<String> {
    let len = cursor.read_u32::<BigEndian>()? as usize;
    read_unicode_string_from(cursor, len)
}

fn read_unicode_string_from(cursor: &mut Cursor<&[u8]>, len: usize) -> io::Result<String> {
    if len == 0 {
        return Ok(String::new());
    }
    if len > MAX_DESCRIPTOR_STRING_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "descriptor string too long",
        ));
    }
    let mut buf = vec![0u16; len];
    for val in buf.iter_mut() {
        *val = cursor.read_u16::<BigEndian>()?;
    }
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(String::from_utf16_lossy(&buf))
}

/// Read a Photoshop ID: if length prefix is 0, read 4 bytes as ASCII.
/// Otherwise read `len` bytes as ASCII string.
pub(crate) fn read_id(cursor: &mut Cursor<&[u8]>) -> io::Result<String> {
    let len = cursor.read_u32::<BigEndian>()? as usize;
    read_id_from(cursor, len as u32)
}

fn read_id_from(cursor: &mut Cursor<&[u8]>, len: u32) -> io::Result<String> {
    let actual_len = if len == 0 { 4 } else { len as usize };
    if actual_len > MAX_DESCRIPTOR_STRING_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "descriptor id too long",
        ));
    }
    let mut buf = vec![0u8; actual_len];
    cursor.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

pub(crate) fn read_type_tag(cursor: &mut Cursor<&[u8]>) -> io::Result<String> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Read a `UntF` value: a 4-byte unit tag (e.g. `#Prc`, `#Pxl`) followed by a
/// big-endian f64. The unit tag is consumed but not interpreted — the caller
/// knows the expected unit from the key.
fn read_unit_float(cursor: &mut Cursor<&[u8]>) -> io::Result<f64> {
    let mut _unit = [0u8; 4];
    cursor.read_exact(&mut _unit)?;
    cursor.read_f64::<BigEndian>()
}

fn skip_descriptor_value(
    cursor: &mut Cursor<&[u8]>,
    type_tag: &str,
    depth: usize,
) -> io::Result<()> {
    if depth > MAX_DESCRIPTOR_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "descriptor nesting too deep",
        ));
    }
    match type_tag {
        "bool" => {
            cursor.read_u8()?;
        }
        "long" => {
            cursor.read_i32::<BigEndian>()?;
        }
        "doub" => {
            cursor.read_f64::<BigEndian>()?;
        }
        "TEXT" => {
            read_unicode_string(cursor)?;
        }
        "enum" => {
            read_id(cursor)?;
            read_id(cursor)?;
        }
        "UntF" => {
            let mut _unit = [0u8; 4];
            cursor.read_exact(&mut _unit)?;
            cursor.read_f64::<BigEndian>()?;
        }
        "VlLs" => {
            let count = cursor.read_u32::<BigEndian>()? as usize;
            for _ in 0..count {
                let tag = read_type_tag(cursor)?;
                skip_descriptor_value(cursor, &tag, depth + 1)?;
            }
        }
        "Objc" => {
            read_unicode_string(cursor)?;
            read_id(cursor)?;
            let count = cursor.read_u32::<BigEndian>()? as usize;
            for _ in 0..count {
                read_id(cursor)?;
                let tag = read_type_tag(cursor)?;
                skip_descriptor_value(cursor, &tag, depth + 1)?;
            }
        }
        "tdta" => {
            let len = cursor.read_u32::<BigEndian>()? as usize;
            if len > MAX_DESCRIPTOR_STRING_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "descriptor tdta too long",
                ));
            }
            let mut buf = vec![0u8; len];
            cursor.read_exact(&mut buf)?;
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown descriptor type tag: {type_tag}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use brushkit_fixture::{
        write_bool, write_brvr, write_doub_ostype, write_key, write_unit_float,
    };
    use byteorder::WriteBytesExt;

    fn build_test_desc_block(name: &str) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();

        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");

        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");

        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();

        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");

        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Nm  ");
        buf.extend_from_slice(b"TEXT");

        let utf16: Vec<u16> = name.encode_utf16().collect();
        buf.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            buf.write_u16::<BigEndian>(*ch).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();

        buf
    }

    fn build_computed_desc_block(name: &str) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(2).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        let utf16: Vec<u16> = name.encode_utf16().collect();
        buf.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            buf.write_u16::<BigEndian>(*ch).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(13).unwrap();
        buf.extend_from_slice(b"computedBrush");
        buf.write_u32::<BigEndian>(5).unwrap();
        write_unit_float(&mut buf, b"Dmtr", b"#Pxl", 30.0);
        write_unit_float(&mut buf, b"Hrdn", b"#Prc", 80.0);
        write_unit_float(&mut buf, b"Angl", b"#Ang", 45.0);
        write_unit_float(&mut buf, b"Rndn", b"#Prc", 60.0);
        write_unit_float(&mut buf, b"Spcn", b"#Prc", 25.0);

        buf
    }

    fn build_sampled_shape_desc_block(name: &str, angl: f64, rndn: f64) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(2).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        let utf16: Vec<u16> = name.encode_utf16().collect();
        buf.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            buf.write_u16::<BigEndian>(*ch).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(12).unwrap();
        buf.extend_from_slice(b"sampledBrush");
        buf.write_u32::<BigEndian>(3).unwrap();
        write_unit_float(&mut buf, b"Dmtr", b"#Pxl", 30.0);
        write_unit_float(&mut buf, b"Angl", b"#Ang", angl);
        write_unit_float(&mut buf, b"Rndn", b"#Prc", rndn);

        buf
    }

    fn build_flip_jitter_desc_block(use_tip_dynamics: bool) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(3).unwrap();

        write_bool(&mut buf, b"useTipDynamics", use_tip_dynamics);
        write_bool(&mut buf, b"flipX", true);
        write_bool(&mut buf, b"flipY", true);

        buf
    }

    fn build_roundness_dynamics_desc_block(use_tip_dynamics: bool) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(3).unwrap();

        write_bool(&mut buf, b"useTipDynamics", use_tip_dynamics);
        write_brvr(&mut buf, b"roundnessDynamics", 0, 53.0);
        write_key(&mut buf, b"minimumRoundness");
        buf.extend_from_slice(b"UntF");
        buf.extend_from_slice(b"#Prc");
        buf.write_f64::<BigEndian>(25.0).unwrap();

        buf
    }

    fn build_paint_dynamics_jitter_desc_block(use_paint_dynamics: bool) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(4).unwrap();

        write_bool(&mut buf, b"usePaintDynamics", use_paint_dynamics);
        write_brvr(&mut buf, b"prVr", 0, 41.0);
        write_brvr(&mut buf, b"wtVr", 0, 7.0);
        write_brvr(&mut buf, b"mxVr", 0, 3.0);

        buf
    }

    fn build_sampled_flip_desc_block() -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(12).unwrap();
        buf.extend_from_slice(b"sampledBrush");
        buf.write_u32::<BigEndian>(2).unwrap();
        write_unit_float(&mut buf, b"Dmtr", b"#Pxl", 30.0);
        write_bool(&mut buf, b"flipX", true);

        buf
    }

    fn build_brush_projection_desc_block(value: bool) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(1).unwrap();

        write_bool(&mut buf, b"brushProjection", value);

        buf
    }

    fn build_dynamics_desc_block(
        name: &str,
        use_tip_dynamics: bool,
        use_scatter: bool,
        use_paint_dynamics: bool,
        sz_control: i32,
        op_control: i32,
    ) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(9).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        let utf16: Vec<u16> = name.encode_utf16().collect();
        buf.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            buf.write_u16::<BigEndian>(*ch).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();

        write_bool(&mut buf, b"useTipDynamics", use_tip_dynamics);
        write_bool(&mut buf, b"useScatter", use_scatter);
        write_bool(&mut buf, b"usePaintDynamics", use_paint_dynamics);

        write_brvr(&mut buf, b"szVr", sz_control, 50.0);
        write_brvr(&mut buf, b"opVr", op_control, 35.0);
        write_brvr(&mut buf, b"angleDynamics", 6, 40.0);
        write_brvr(&mut buf, b"scatterDynamics", 0, 80.0);

        write_key(&mut buf, b"minimumDiameter");
        buf.extend_from_slice(b"UntF");
        buf.extend_from_slice(b"#Prc");
        buf.write_f64::<BigEndian>(40.0).unwrap();

        buf
    }

    fn build_scatter_count_desc_block(use_scatter: bool) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(5).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        let utf16: Vec<u16> = "Scatter".encode_utf16().collect();
        buf.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            buf.write_u16::<BigEndian>(*ch).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();

        write_bool(&mut buf, b"useScatter", use_scatter);
        write_doub_ostype(&mut buf, b"Cnt ", 3.0);
        write_bool(&mut buf, b"bothAxes", false);
        write_brvr(&mut buf, b"countDynamics", 0, 25.0);

        buf
    }

    fn write_enum(buf: &mut Vec<u8>, key: &[u8], enum_type: &[u8], enum_value: &[u8]) {
        write_key(buf, key);
        buf.extend_from_slice(b"enum");
        write_key(buf, enum_type);
        write_key(buf, enum_value);
    }

    fn write_long(buf: &mut Vec<u8>, key: &[u8], val: i32) {
        write_key(buf, key);
        buf.extend_from_slice(b"long");
        buf.write_i32::<BigEndian>(val).unwrap();
    }

    fn write_text(buf: &mut Vec<u8>, key: &[u8], text: &str) {
        write_key(buf, key);
        buf.extend_from_slice(b"TEXT");
        let utf16: Vec<u16> = text.encode_utf16().collect();
        buf.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            buf.write_u16::<BigEndian>(*ch).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();
    }

    fn write_txtr(buf: &mut Vec<u8>, name: &str, id: &str) {
        write_key(buf, b"Txtr");
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        write_key(buf, b"Ptrn");
        buf.write_u32::<BigEndian>(2).unwrap();
        write_text(buf, b"Nm  ", name);
        write_text(buf, b"Idnt", id);
    }

    fn build_texture_desc_block(use_texture: bool) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(1).unwrap();

        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(13).unwrap();

        write_text(&mut buf, b"Nm  ", "Textured");

        write_bool(&mut buf, b"useTexture", use_texture);
        write_bool(&mut buf, b"TxtC", true);
        write_bool(&mut buf, b"interpretation", true);
        write_enum(&mut buf, b"textureBlendMode", b"BlnM", b"height");
        write_key(&mut buf, b"textureDepth");
        buf.extend_from_slice(b"UntF");
        buf.extend_from_slice(b"#Prc");
        buf.write_f64::<BigEndian>(100.0).unwrap();
        write_key(&mut buf, b"minimumDepth");
        buf.extend_from_slice(b"UntF");
        buf.extend_from_slice(b"#Prc");
        buf.write_f64::<BigEndian>(0.0).unwrap();
        write_brvr(&mut buf, b"textureDepthDynamics", 0, 0.0);
        write_txtr(
            &mut buf,
            "Kafelki z wzorem drzewa 4",
            "92150a0d-bc3e-9349-adc8-869fb042a4b0",
        );
        write_key(&mut buf, b"textureScale");
        buf.extend_from_slice(b"UntF");
        buf.extend_from_slice(b"#Prc");
        buf.write_f64::<BigEndian>(100.0).unwrap();
        write_bool(&mut buf, b"InvT", false);
        write_long(&mut buf, b"textureBrightness", 0);
        write_long(&mut buf, b"textureContrast", 0);

        buf
    }

    #[test]
    fn texture_section_is_parsed() {
        let data = build_texture_desc_block(true);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert_eq!(
            d.texture_pattern_id.as_deref(),
            Some("92150a0d-bc3e-9349-adc8-869fb042a4b0")
        );
        assert_eq!(
            d.texture_pattern_name.as_deref(),
            Some("Kafelki z wzorem drzewa 4")
        );
        assert_eq!(d.texture_depth_pct, Some(100.0));
        assert_eq!(d.texture_minimum_depth_pct, Some(0.0));
        assert_eq!(d.texture_scale_pct, Some(100.0));
        assert_eq!(d.texture_invert, Some(false));
        assert_eq!(d.texture_each_tip, Some(true));
        assert_eq!(d.texture_blend_mode.as_deref(), Some("height"));
        assert_eq!(d.texture_depth_jitter_pct, Some(0.0));
        assert_eq!(d.texture_brightness, Some(0));
        assert_eq!(d.texture_contrast, Some(0));
    }

    #[test]
    fn texture_gated_off_when_use_texture_false() {
        let data = build_texture_desc_block(false);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert!(d.texture_pattern_id.is_none());
        assert!(d.texture_pattern_name.is_none());
        assert!(d.texture_depth_pct.is_none());
        assert!(d.texture_minimum_depth_pct.is_none());
        assert!(d.texture_scale_pct.is_none());
        assert!(d.texture_invert.is_none());
        assert!(d.texture_each_tip.is_none());
        assert!(d.texture_blend_mode.is_none());
        assert!(d.texture_depth_jitter_pct.is_none());
        assert!(d.texture_brightness.is_none());
        assert!(d.texture_contrast.is_none());
    }

    #[test]
    fn no_texture_keys_yields_none() {
        let data = build_test_desc_block("x");
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert!(d.texture_pattern_id.is_none());
        assert!(d.texture_pattern_name.is_none());
        assert!(d.texture_depth_pct.is_none());
        assert!(d.texture_minimum_depth_pct.is_none());
        assert!(d.texture_scale_pct.is_none());
        assert!(d.texture_invert.is_none());
        assert!(d.texture_each_tip.is_none());
        assert!(d.texture_blend_mode.is_none());
        assert!(d.texture_depth_jitter_pct.is_none());
        assert!(d.texture_brightness.is_none());
        assert!(d.texture_contrast.is_none());
    }

    #[test]
    fn scatter_count_and_both_axes_are_parsed() {
        let data = build_scatter_count_desc_block(true);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert_eq!(d.scatter_count, Some(3.0));
        assert_eq!(d.scatter_both_axes, Some(false));
    }

    #[test]
    fn scatter_count_gated_off_when_use_scatter_false() {
        let data = build_scatter_count_desc_block(false);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert!(d.scatter_count.is_none());
        assert!(d.scatter_both_axes.is_none());
    }

    #[test]
    fn count_jitter_is_parsed() {
        let data = build_scatter_count_desc_block(true);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].descriptor.count_jitter_pct, Some(25.0));
    }

    #[test]
    fn count_jitter_gated_off_when_use_scatter_false() {
        let data = build_scatter_count_desc_block(false);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.count_jitter_pct.is_none());
    }

    #[test]
    fn dynamics_blocks_are_parsed() {
        let data = build_dynamics_desc_block("Dynamic", true, true, false, 0, 0);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert_eq!(d.scatter_amount_pct, Some(80.0));
        assert_eq!(d.size_jitter_pct, Some(50.0));
        assert_eq!(d.angle_control, Some(6));
        assert_eq!(d.angle_jitter_pct, Some(40.0));
        assert_eq!(d.minimum_diameter_pct, Some(40.0));
    }

    #[test]
    fn dynamics_gated_off_when_enabling_bool_false() {
        let data = build_dynamics_desc_block("Gated", false, false, false, 0, 0);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert!(d.scatter_amount_pct.is_none());
        assert!(d.size_jitter_pct.is_none());
        assert!(d.angle_control.is_none());
        assert!(d.angle_jitter_pct.is_none());
        assert!(d.minimum_diameter_pct.is_none());
    }

    #[test]
    fn flip_jitter_surfaces_under_tip_dynamics_gate() {
        let data = build_flip_jitter_desc_block(true);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].descriptor.flip_x_jitter, Some(true));
        assert_eq!(infos[0].descriptor.flip_y_jitter, Some(true));
    }

    #[test]
    fn flip_jitter_gated_off_when_use_tip_dynamics_false() {
        let data = build_flip_jitter_desc_block(false);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.flip_x_jitter.is_none());
        assert!(infos[0].descriptor.flip_y_jitter.is_none());
    }

    #[test]
    fn roundness_dynamics_surface_under_tip_dynamics_gate() {
        let data = build_roundness_dynamics_desc_block(true);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].descriptor.roundness_jitter_pct, Some(53.0));
        assert_eq!(infos[0].descriptor.minimum_roundness_pct, Some(25.0));
    }

    #[test]
    fn roundness_dynamics_gated_off_when_use_tip_dynamics_false() {
        let data = build_roundness_dynamics_desc_block(false);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.roundness_jitter_pct.is_none());
        assert!(infos[0].descriptor.minimum_roundness_pct.is_none());
    }

    #[test]
    fn paint_dynamics_jitters_surface_under_paint_dynamics_gate() {
        let data = build_paint_dynamics_jitter_desc_block(true);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].descriptor.flow_jitter_pct, Some(41.0));
        assert_eq!(infos[0].descriptor.wetness_jitter_pct, Some(7.0));
        assert_eq!(infos[0].descriptor.mix_jitter_pct, Some(3.0));
    }

    #[test]
    fn paint_dynamics_jitters_gated_off_when_paint_dynamics_false() {
        let data = build_paint_dynamics_jitter_desc_block(false);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.flow_jitter_pct.is_none());
        assert!(infos[0].descriptor.wetness_jitter_pct.is_none());
        assert!(infos[0].descriptor.mix_jitter_pct.is_none());
    }

    #[test]
    fn sampled_tip_flip_is_independent_of_jitter_flips() {
        let data = build_sampled_flip_desc_block();
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert_eq!(d.tip_flip_x, Some(true));
        assert!(d.tip_flip_y.is_none());
        assert!(d.flip_x_jitter.is_none());
        assert!(d.flip_y_jitter.is_none());
    }

    #[test]
    fn brush_projection_is_parsed_when_present() {
        let data = build_brush_projection_desc_block(true);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.brush_projection);
    }

    #[test]
    fn brush_projection_defaults_false_when_absent() {
        let data = build_test_desc_block("NoProjection");
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(!infos[0].descriptor.brush_projection);
    }

    #[test]
    fn pressure_controls_are_parsed() {
        let data = build_dynamics_desc_block("Pressure", true, false, true, 2, 2);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert_eq!(d.pressure_size_control, Some(2));
        assert_eq!(d.pressure_opacity_control, Some(2));
    }

    #[test]
    fn pressure_controls_gated_off_when_enabling_bool_false() {
        let data = build_dynamics_desc_block("Gated", false, false, false, 2, 2);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert!(d.pressure_size_control.is_none());
        assert!(d.pressure_opacity_control.is_none());
    }

    #[test]
    fn opacity_jitter_is_parsed() {
        let data = build_dynamics_desc_block("Paint", false, false, true, 0, 0);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].descriptor.opacity_jitter_pct, Some(35.0));
    }

    #[test]
    fn opacity_jitter_gated_off_when_paint_dynamics_false() {
        let data = build_dynamics_desc_block("Gated", false, false, false, 0, 0);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.opacity_jitter_pct.is_none());
    }

    #[test]
    fn absent_paint_dynamics_yields_no_opacity_control() {
        let data = build_test_desc_block("x");
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.pressure_opacity_control.is_none());
    }

    #[test]
    fn no_dynamics_blocks_yields_none() {
        let data = build_test_desc_block("x");
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert!(d.scatter_amount_pct.is_none());
        assert!(d.size_jitter_pct.is_none());
        assert!(d.angle_control.is_none());
        assert!(d.angle_jitter_pct.is_none());
    }

    #[test]
    fn computed_brush_geometry_is_parsed() {
        let data = build_computed_desc_block("Procedural");
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let info = &infos[0];
        assert_eq!(info.name, "Procedural");
        assert!(info.sampled_data_uuid.is_none());
        assert_eq!(info.descriptor.spacing_pct, Some(25.0));
        assert_eq!(
            info.descriptor.computed,
            Some(crate::ComputedGeometry {
                diameter_px: Some(30.0),
                hardness_pct: Some(80.0),
                angle_deg: Some(45.0),
                roundness_pct: Some(60.0),
            })
        );
        assert_eq!(info.descriptor.diameter_px, Some(30.0));
    }

    #[test]
    fn sampled_brush_has_no_computed_geometry() {
        let data = build_test_desc_block("Sampled");
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        assert!(infos[0].descriptor.computed.is_none());
    }

    #[test]
    fn sampled_brush_shape_is_parsed() {
        let data = build_sampled_shape_desc_block("Sampled", 45.0, 30.0);
        let infos = extract_all_brush_info(&data);
        assert_eq!(infos.len(), 1);
        let d = &infos[0].descriptor;
        assert_eq!(d.shape_angle_deg, Some(45.0));
        assert_eq!(d.shape_roundness_pct, Some(30.0));
        assert_eq!(d.diameter_px, Some(30.0));
        assert!(d.computed.is_none());
    }

    #[test]
    fn test_extract_brush_name_ascii() {
        let data = build_test_desc_block("My Brush");
        let name = extract_brush_name(&data);
        assert_eq!(name, Some("My Brush".to_string()));
    }

    #[test]
    fn test_extract_brush_name_unicode() {
        let data = build_test_desc_block("P\u{0119}dzel");
        let name = extract_brush_name(&data);
        assert_eq!(name, Some("P\u{0119}dzel".to_string()));
    }

    #[test]
    fn test_extract_brush_name_empty() {
        let name = extract_brush_name(&[0, 0, 0]);
        assert_eq!(name, None);
    }

    #[test]
    fn test_read_id_zero_length() {
        let data = [0u8, 0, 0, 0, b'T', b'E', b'S', b'T'];
        let mut cursor = Cursor::new(data.as_slice());
        let id = read_id(&mut cursor).unwrap();
        assert_eq!(id, "TEST");
    }

    #[test]
    fn test_read_id_nonzero_length() {
        let mut data = Vec::new();
        data.write_u32::<BigEndian>(5).unwrap();
        data.extend_from_slice(b"Hello");
        let mut cursor = Cursor::new(data.as_slice());
        let id = read_id(&mut cursor).unwrap();
        assert_eq!(id, "Hello");
    }

    #[test]
    fn test_read_unicode_string() {
        let mut data = Vec::new();
        let text = "Test";
        let utf16: Vec<u16> = text.encode_utf16().collect();
        data.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            data.write_u16::<BigEndian>(*ch).unwrap();
        }
        data.write_u16::<BigEndian>(0).unwrap();
        let mut cursor = Cursor::new(data.as_slice());
        let result = read_unicode_string(&mut cursor).unwrap();
        assert_eq!(result, "Test");
    }

    #[test]
    fn test_malformed_unicode_string_too_long() {
        let data: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(data.as_slice());
        let res = read_unicode_string_from(&mut cursor, MAX_DESCRIPTOR_STRING_LEN + 1);
        assert!(res.is_err());
    }

    #[test]
    fn test_malformed_id_too_long() {
        let data: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(data.as_slice());
        let res = read_id_from(&mut cursor, (MAX_DESCRIPTOR_STRING_LEN + 1) as u32);
        assert!(res.is_err());
    }

    #[test]
    fn test_malformed_tdta_too_long() {
        let mut data = Vec::new();
        data.write_u32::<BigEndian>((MAX_DESCRIPTOR_STRING_LEN + 1) as u32)
            .unwrap();
        let mut cursor = Cursor::new(data.as_slice());
        let res = skip_descriptor_value(&mut cursor, "tdta", 0);
        assert!(res.is_err());
    }

    #[test]
    fn test_malformed_recursion_too_deep() {
        let data: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(data.as_slice());
        let err = skip_descriptor_value(&mut cursor, "Objc", MAX_DESCRIPTOR_DEPTH + 1)
            .expect_err("expected rejection at excessive depth");
        assert!(err.to_string().contains("nesting too deep"));
    }

    #[test]
    fn alt_format_zero_class_name_nonzero_class_id() {
        let mut data = Vec::new();
        data.write_u32::<BigEndian>(16).unwrap();
        data.write_u32::<BigEndian>(0).unwrap();
        data.write_u32::<BigEndian>(4).unwrap();
        data.extend_from_slice(b"null");
        data.write_u32::<BigEndian>(0).unwrap();

        let res = extract_all_brush_info_inner(&data);
        assert!(res.is_ok(), "expected Ok, got {res:?}");
        assert!(res.unwrap().is_empty());
    }
}
