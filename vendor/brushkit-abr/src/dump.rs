//! Raw-descriptor capturing walker for corpus characterization. Unlike
//! `crate::descriptor::extract_all_brush_info` (private), which keeps only the
//! fields a mapper needs, this walker records *every* `(key, tag,
//! value)` it encounters so an operator can witness the full descriptor tree of
//! a real brush corpus and rewrite the mapping table from observed values.

use crate::descriptor::{read_id, read_type_tag, read_unicode_string, MAX_DESCRIPTOR_DEPTH};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{self, Cursor, Read};

/// The full raw-descriptor dump of a desc block.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DescriptorDump {
    pub presets: Vec<PresetDump>,
    /// Set when an unknown type tag stopped the walk early.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub unknown_type_stop: Option<UnknownTypeStop>,
}

/// Where an unknown type tag ended the walk.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UnknownTypeStop {
    pub preset_index: usize,
    pub depth: usize,
    pub key: String,
    pub tag: String,
}

/// One captured brush preset (or one captured top-level / list item).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PresetDump {
    pub index: usize,
    pub items: Vec<DumpItem>,
}

/// A single captured `(key, type tag, value)` node.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DumpItem {
    pub key: String,
    /// The 4-char type tag as read from the wire ("TEXT", "UntF", …).
    pub r#type: String,
    pub depth: usize,
    pub value: DumpValue,
}

/// A captured descriptor value, tagged by wire type.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum DumpValue {
    Bool(bool),
    Long(i32),
    Doub(f64),
    Text(String),
    Enum {
        enum_type: String,
        enum_value: String,
    },
    UnitFloat {
        unit: String,
        number: f64,
    },
    List(Vec<DumpValue>),
    Objc {
        class_id: String,
        items: Vec<DumpItem>,
    },
    Tdta {
        tdta_len: usize,
    },
    Unknown {
        tag: String,
    },
}

const MAX_DESCRIPTOR_STRING_LEN: usize = 1024 * 1024;

/// Capture the full raw descriptor tree from a desc block.
///
/// Total: never panics. On any I/O error or depth-guard trip mid-walk, returns
/// whatever was successfully captured so far. On an unknown type tag, records
/// the stop location, keeps presets already fully walked, and stops the walk
/// (an unsized unknown value poisons the cursor for everything after it).
pub fn dump_descriptors(data: &[u8]) -> DescriptorDump {
    let mut walker = Walker {
        dump: DescriptorDump::default(),
        next_index: 0,
    };
    let mut cursor = Cursor::new(data);
    let _ = walker.walk_framed(&mut cursor);
    walker.dump
}

struct Walker {
    dump: DescriptorDump,
    next_index: usize,
}

impl Walker {
    fn stopped(&self) -> bool {
        self.dump.unknown_type_stop.is_some()
    }

    fn walk_framed(&mut self, cursor: &mut Cursor<&[u8]>) -> io::Result<()> {
        let _fmt_ver = cursor.read_u32::<BigEndian>()?;

        let peek_pos = cursor.position();
        let first_u32 = cursor.read_u32::<BigEndian>()?;

        if first_u32 == 0 {
            let next_u32 = cursor.read_u32::<BigEndian>()?;
            if next_u32 == 0 {
                let mut ci = [0u8; 4];
                cursor.read_exact(&mut ci)?;
            } else {
                cursor.set_position(peek_pos + 4);
                let _class_id = read_id_from_len(cursor, next_u32)?;
            }
        } else if first_u32 == 1 {
            let char_val = cursor.read_u16::<BigEndian>()?;
            if char_val == 0 {
                let _class_id = read_id(cursor)?;
            } else {
                cursor.set_position(peek_pos + 4);
                let _class_name = read_unicode_string_len(cursor, 0)?;
                let _class_id = read_id(cursor)?;
            }
        } else {
            let _class_name = read_unicode_string_len(cursor, first_u32 as usize)?;
            let _class_id = read_id(cursor)?;
        }

        let item_count = cursor.read_u32::<BigEndian>()?;

        for _ in 0..item_count {
            if self.stopped() {
                return Ok(());
            }
            let key = read_id(cursor)?;
            let type_tag = read_type_tag(cursor)?;

            if key == "Brsh" && type_tag == "VlLs" {
                let vl_count = cursor.read_u32::<BigEndian>()?;
                for _ in 0..vl_count {
                    if self.stopped() {
                        return Ok(());
                    }
                    let elem_tag = read_type_tag(cursor)?;
                    if elem_tag == "Objc" {
                        let index = self.alloc_index();
                        let items = self.read_objc_items(cursor, 0, index)?;
                        self.dump.presets.push(PresetDump { index, items });
                    } else {
                        let index = self.alloc_index();
                        let value = self.read_value(cursor, &elem_tag, 0, "Brsh", index)?;
                        self.dump.presets.push(PresetDump {
                            index,
                            items: vec![DumpItem {
                                key: "Brsh".to_string(),
                                r#type: elem_tag,
                                depth: 0,
                                value,
                            }],
                        });
                    }
                }
            } else {
                let index = self.alloc_index();
                let value = self.read_value(cursor, &type_tag, 0, &key, index)?;
                self.dump.presets.push(PresetDump {
                    index,
                    items: vec![DumpItem {
                        key,
                        r#type: type_tag,
                        depth: 0,
                        value,
                    }],
                });
            }
        }

        Ok(())
    }

    fn alloc_index(&mut self) -> usize {
        let i = self.next_index;
        self.next_index += 1;
        i
    }

    fn read_objc_items(
        &mut self,
        cursor: &mut Cursor<&[u8]>,
        depth: usize,
        preset_index: usize,
    ) -> io::Result<Vec<DumpItem>> {
        let _class_name = read_unicode_string(cursor)?;
        let _class_id = read_id(cursor)?;
        let item_count = cursor.read_u32::<BigEndian>()?;

        let mut items = Vec::new();
        for _ in 0..item_count {
            let key = read_id(cursor)?;
            let type_tag = read_type_tag(cursor)?;
            let value = self.read_value(cursor, &type_tag, depth, &key, preset_index)?;
            items.push(DumpItem {
                key,
                r#type: type_tag,
                depth,
                value,
            });
            if self.stopped() {
                break;
            }
        }
        Ok(items)
    }

    fn read_objc_class(&mut self, cursor: &mut Cursor<&[u8]>) -> io::Result<String> {
        let _class_name = read_unicode_string(cursor)?;
        read_id(cursor)
    }

    fn read_value(
        &mut self,
        cursor: &mut Cursor<&[u8]>,
        type_tag: &str,
        depth: usize,
        key: &str,
        preset_index: usize,
    ) -> io::Result<DumpValue> {
        if depth > MAX_DESCRIPTOR_DEPTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "descriptor nesting too deep",
            ));
        }
        let value = match type_tag {
            "bool" => DumpValue::Bool(cursor.read_u8()? != 0),
            "long" => DumpValue::Long(cursor.read_i32::<BigEndian>()?),
            "doub" => DumpValue::Doub(cursor.read_f64::<BigEndian>()?),
            "TEXT" => DumpValue::Text(read_unicode_string(cursor)?),
            "enum" => {
                let enum_type = read_id(cursor)?;
                let enum_value = read_id(cursor)?;
                DumpValue::Enum {
                    enum_type,
                    enum_value,
                }
            }
            "UntF" => {
                let mut unit = [0u8; 4];
                cursor.read_exact(&mut unit)?;
                let number = cursor.read_f64::<BigEndian>()?;
                DumpValue::UnitFloat {
                    unit: String::from_utf8_lossy(&unit).to_string(),
                    number,
                }
            }
            "VlLs" => {
                let count = cursor.read_u32::<BigEndian>()? as usize;
                let mut list = Vec::new();
                for _ in 0..count {
                    let elem_tag = read_type_tag(cursor)?;
                    let v = self.read_value(cursor, &elem_tag, depth + 1, key, preset_index)?;
                    list.push(v);
                    if self.stopped() {
                        break;
                    }
                }
                DumpValue::List(list)
            }
            "Objc" => {
                let class_id = self.read_objc_class(cursor)?;
                let items = self.read_objc_items_after_class(cursor, depth + 1, preset_index)?;
                DumpValue::Objc { class_id, items }
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
                DumpValue::Tdta { tdta_len: len }
            }
            _ => {
                self.dump.unknown_type_stop = Some(UnknownTypeStop {
                    preset_index,
                    depth,
                    key: key.to_string(),
                    tag: type_tag.to_string(),
                });
                DumpValue::Unknown {
                    tag: type_tag.to_string(),
                }
            }
        };
        Ok(value)
    }

    fn read_objc_items_after_class(
        &mut self,
        cursor: &mut Cursor<&[u8]>,
        depth: usize,
        preset_index: usize,
    ) -> io::Result<Vec<DumpItem>> {
        let item_count = cursor.read_u32::<BigEndian>()?;
        let mut items = Vec::new();
        for _ in 0..item_count {
            let key = read_id(cursor)?;
            let type_tag = read_type_tag(cursor)?;
            let value = self.read_value(cursor, &type_tag, depth, &key, preset_index)?;
            items.push(DumpItem {
                key,
                r#type: type_tag,
                depth,
                value,
            });
            if self.stopped() {
                break;
            }
        }
        Ok(items)
    }
}

fn read_unicode_string_len(cursor: &mut Cursor<&[u8]>, len: usize) -> io::Result<String> {
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

fn read_id_from_len(cursor: &mut Cursor<&[u8]>, len: u32) -> io::Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::WriteBytesExt;

    fn write_id_4cc(buf: &mut Vec<u8>, code: &[u8; 4]) {
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(code);
    }

    fn write_text_value(buf: &mut Vec<u8>, text: &str) {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        buf.write_u32::<BigEndian>(utf16.len() as u32 + 1).unwrap();
        for ch in &utf16 {
            buf.write_u16::<BigEndian>(*ch).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();
    }

    fn write_header_and_brsh_list(buf: &mut Vec<u8>, n: u32) {
        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        write_id_4cc(buf, b"null");
        buf.write_u32::<BigEndian>(1).unwrap();
        write_id_4cc(buf, b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(n).unwrap();
    }

    fn write_preset_objc_open(buf: &mut Vec<u8>, class_id: &[u8], n_items: u32) {
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(class_id.len() as u32).unwrap();
        buf.extend_from_slice(class_id);
        buf.write_u32::<BigEndian>(n_items).unwrap();
    }

    const TDTA_LEN: usize = 7;

    fn build_rich_block() -> Vec<u8> {
        let mut buf = Vec::new();
        write_header_and_brsh_list(&mut buf, 1);
        write_preset_objc_open(&mut buf, b"brushPreset", 3);

        write_id_4cc(&mut buf, b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        write_text_value(&mut buf, "Hi");

        write_id_4cc(&mut buf, b"Brsh");
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        let cid = b"sampledBrush";
        buf.write_u32::<BigEndian>(cid.len() as u32).unwrap();
        buf.extend_from_slice(cid);
        buf.write_u32::<BigEndian>(4).unwrap();

        write_id_4cc(&mut buf, b"Spcn");
        buf.extend_from_slice(b"UntF");
        buf.extend_from_slice(b"#Prc");
        buf.write_f64::<BigEndian>(25.0).unwrap();

        write_id_4cc(&mut buf, b"flip");
        buf.extend_from_slice(b"bool");
        buf.write_u8(1).unwrap();

        write_id_4cc(&mut buf, b"mode");
        buf.extend_from_slice(b"enum");
        write_id_4cc(&mut buf, b"blnM");
        write_id_4cc(&mut buf, b"Nrml");

        write_id_4cc(&mut buf, b"list");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(2).unwrap();
        buf.extend_from_slice(b"doub");
        buf.write_f64::<BigEndian>(1.5).unwrap();
        buf.extend_from_slice(b"doub");
        buf.write_f64::<BigEndian>(2.5).unwrap();

        write_id_4cc(&mut buf, b"extD");
        buf.extend_from_slice(b"tdta");
        buf.write_u32::<BigEndian>(TDTA_LEN as u32).unwrap();
        buf.extend_from_slice(&[0xABu8; TDTA_LEN]);

        buf
    }

    #[test]
    fn test_full_capture() {
        let data = build_rich_block();
        let dump = dump_descriptors(&data);
        assert!(dump.unknown_type_stop.is_none());
        assert_eq!(dump.presets.len(), 1);
        let preset = &dump.presets[0];
        assert_eq!(preset.index, 0);
        assert_eq!(preset.items.len(), 3);

        assert_eq!(preset.items[0].key, "Nm  ");
        assert_eq!(preset.items[0].depth, 0);
        assert!(matches!(preset.items[0].value, DumpValue::Text(ref s) if s == "Hi"));

        let brsh = &preset.items[1];
        assert_eq!(brsh.depth, 0);
        let (class_id, items) = match &brsh.value {
            DumpValue::Objc { class_id, items } => (class_id, items),
            other => panic!("expected Objc, got {other:?}"),
        };
        assert_eq!(class_id, "sampledBrush");
        assert_eq!(items.len(), 4);
        for it in items {
            assert_eq!(it.depth, 1, "nested Objc items are depth 1");
        }

        assert_eq!(items[0].key, "Spcn");
        assert!(matches!(
            items[0].value,
            DumpValue::UnitFloat { ref unit, number } if unit == "#Prc" && number == 25.0
        ));
        assert!(matches!(items[1].value, DumpValue::Bool(true)));
        assert!(matches!(
            items[2].value,
            DumpValue::Enum { ref enum_type, ref enum_value }
                if enum_type == "blnM" && enum_value == "Nrml"
        ));
        match &items[3].value {
            DumpValue::List(v) => {
                assert_eq!(v.len(), 2);
                assert!(matches!(v[0], DumpValue::Doub(d) if d == 1.5));
                assert!(matches!(v[1], DumpValue::Doub(d) if d == 2.5));
            }
            other => panic!("expected List, got {other:?}"),
        }

        assert_eq!(preset.items[2].key, "extD");
        assert!(matches!(
            preset.items[2].value,
            DumpValue::Tdta { tdta_len } if tdta_len == TDTA_LEN
        ));
    }

    #[test]
    fn test_unknown_tag_record_and_stop() {
        let mut buf = Vec::new();
        write_header_and_brsh_list(&mut buf, 2);

        write_preset_objc_open(&mut buf, b"brushPreset", 1);
        write_id_4cc(&mut buf, b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        write_text_value(&mut buf, "A");

        write_preset_objc_open(&mut buf, b"brushPreset", 2);
        write_id_4cc(&mut buf, b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        write_text_value(&mut buf, "B");
        write_id_4cc(&mut buf, b"bad ");
        buf.extend_from_slice(b"Xyz1");

        let dump = dump_descriptors(&buf);

        assert_eq!(dump.presets.len(), 2);
        assert_eq!(dump.presets[0].index, 0);
        assert!(matches!(
            dump.presets[0].items[0].value,
            DumpValue::Text(ref s) if s == "A"
        ));

        let stop = dump.unknown_type_stop.as_ref().expect("stop recorded");
        assert_eq!(stop.preset_index, 1);
        assert_eq!(stop.tag, "Xyz1");
        assert_eq!(stop.key, "bad ");

        let p1 = &dump.presets[1];
        let last = p1.items.last().unwrap();
        assert!(matches!(last.value, DumpValue::Unknown { ref tag } if tag == "Xyz1"));
    }

    fn nested_objc_dump(levels: usize) -> DescriptorDump {
        let mut buf = Vec::new();
        write_header_and_brsh_list(&mut buf, 1);
        write_preset_objc_open(&mut buf, b"brushPreset", 1);
        for _ in 0..levels {
            write_id_4cc(&mut buf, b"deep");
            buf.extend_from_slice(b"Objc");
            buf.write_u32::<BigEndian>(1).unwrap();
            buf.write_u16::<BigEndian>(0).unwrap();
            write_id_4cc(&mut buf, b"x   ");
            buf.write_u32::<BigEndian>(1).unwrap();
        }
        write_id_4cc(&mut buf, b"end ");
        buf.extend_from_slice(b"bool");
        buf.write_u8(0).unwrap();
        dump_descriptors(&buf)
    }

    fn deepest_objc_depth(items: &[DumpItem]) -> usize {
        items
            .iter()
            .map(|it| match &it.value {
                DumpValue::Objc { items, .. } => it.depth.max(deepest_objc_depth(items)),
                _ => it.depth,
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn test_depth_guard() {
        let dump = nested_objc_dump(MAX_DESCRIPTOR_DEPTH + 1);
        assert!(
            dump.presets.is_empty(),
            "one past the ceiling the guard must abandon the preset, not return \
             a partial one: got {} preset(s)",
            dump.presets.len()
        );
        assert!(
            dump.unknown_type_stop.is_none(),
            "a depth bail is not an unknown-tag stop"
        );

        let runaway = nested_objc_dump(MAX_DESCRIPTOR_DEPTH + 10);
        assert!(
            runaway.presets.is_empty(),
            "deep nesting must stay bounded: got {} preset(s)",
            runaway.presets.len()
        );
    }

    #[test]
    fn test_depth_guard_keeps_nesting_inside_the_ceiling() {
        const LEVELS: usize = MAX_DESCRIPTOR_DEPTH;
        let dump = nested_objc_dump(LEVELS);
        assert_eq!(
            dump.presets.len(),
            1,
            "nesting inside the ceiling must still capture its preset"
        );
        assert_eq!(
            deepest_objc_depth(&dump.presets[0].items),
            LEVELS,
            "the walk must reach the terminal bool at depth {LEVELS}, the \
             deepest level the guard still allows"
        );
    }

    #[test]
    fn test_garbage_tolerance() {
        let dump = dump_descriptors(&[0, 0, 0]);
        assert!(dump.presets.is_empty());
        assert!(dump.unknown_type_stop.is_none());
    }
}
