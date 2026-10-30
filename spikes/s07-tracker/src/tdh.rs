//! Decoding a manifest event's top-level properties from the layout TDH reports
//! (`TRACE_EVENT_INFO`). The Kernel-File, -Registry and -Process events are flat: scalars,
//! NUL-terminated strings, SIDs and length-prefixed binaries. A struct, or an array of anything
//! but fixed-size scalars, ends decoding: later properties read as `None`, never as a guess.

/// `TDH_INTYPE_*` values.
pub mod in_type {
    pub const UNICODE_STRING: u16 = 1;
    pub const ANSI_STRING: u16 = 2;
    pub const INT8: u16 = 3;
    pub const UINT8: u16 = 4;
    pub const INT16: u16 = 5;
    pub const UINT16: u16 = 6;
    pub const INT32: u16 = 7;
    pub const UINT32: u16 = 8;
    pub const INT64: u16 = 9;
    pub const UINT64: u16 = 10;
    pub const FLOAT: u16 = 11;
    pub const DOUBLE: u16 = 12;
    pub const BOOLEAN: u16 = 13;
    pub const BINARY: u16 = 14;
    pub const GUID: u16 = 15;
    pub const POINTER: u16 = 16;
    pub const FILETIME: u16 = 17;
    pub const SYSTEMTIME: u16 = 18;
    pub const SID: u16 = 19;
    pub const HEXINT32: u16 = 20;
    pub const HEXINT64: u16 = 21;
    pub const MANIFEST_COUNTED_STRING: u16 = 22;
    pub const MANIFEST_COUNTED_BINARY: u16 = 25;
    pub const COUNTED_STRING: u16 = 300;
    pub const SIZET: u16 = 308;
    pub const WBEMSID: u16 = 310;
}

/// `PROPERTY_FLAGS` bits this decoder acts on.
pub const PROPERTY_STRUCT: i32 = 1;
pub const PROPERTY_PARAM_LENGTH: i32 = 2;
pub const PROPERTY_PARAM_COUNT: i32 = 4;

#[derive(Clone, Debug)]
pub struct Prop {
    pub name: String,
    pub in_type: u16,
    pub flags: i32,
    /// Element count, or the index of the property holding it (`PROPERTY_PARAM_COUNT`).
    pub count: u16,
    /// Length (characters for strings, bytes otherwise), or the index of the property holding it
    /// (`PROPERTY_PARAM_LENGTH`). 0 = not given (strings are then NUL-terminated).
    pub length: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(u64),
    Str(String),
    /// Binary, SID or array data: only its size is kept.
    Bytes(usize),
}

impl Value {
    pub fn int(&self) -> Option<u64> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// One event's layout: TDH's task name and the top-level properties in order.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub task: String,
    pub props: Vec<Prop>,
}

impl Layout {
    pub fn index(&self, name: &str) -> Option<usize> {
        self.props
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(name))
    }
}

fn scalar_size(in_type: u16, pointer: usize) -> Option<usize> {
    use in_type::{
        BOOLEAN, DOUBLE, FILETIME, FLOAT, GUID, HEXINT32, HEXINT64, INT8, INT16, INT32, INT64,
        POINTER, SIZET, SYSTEMTIME, UINT8, UINT16, UINT32, UINT64,
    };
    Some(match in_type {
        INT8 | UINT8 => 1,
        INT16 | UINT16 => 2,
        INT32 | UINT32 | HEXINT32 | FLOAT | BOOLEAN => 4,
        INT64 | UINT64 | HEXINT64 | DOUBLE | FILETIME => 8,
        GUID | SYSTEMTIME => 16,
        POINTER | SIZET => pointer,
        _ => return None,
    })
}

fn little_endian(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0, |acc, &b| (acc << 8) | u64::from(b))
}

/// Decodes `data` against `layout`. `pointer` is the event's pointer size (4 or 8).
pub fn decode(layout: &Layout, data: &[u8], pointer: usize) -> Vec<Option<Value>> {
    let mut out: Vec<Option<Value>> = vec![None; layout.props.len()];
    let mut at = 0;
    for (i, prop) in layout.props.iter().enumerate() {
        let Some(value) = decode_one(prop, &out, data, &mut at, pointer) else {
            break;
        };
        out[i] = Some(value);
    }
    out
}

fn param(out: &[Option<Value>], index: u16) -> Option<usize> {
    let v = out.get(usize::from(index))?.as_ref()?.int()?;
    usize::try_from(v).ok()
}

fn utf16(units: &[[u8; 2]]) -> String {
    let units: Vec<u16> = units.iter().map(|u| u16::from_le_bytes(*u)).collect();
    String::from_utf16_lossy(&units)
}

fn decode_one(
    prop: &Prop,
    out: &[Option<Value>],
    data: &[u8],
    at: &mut usize,
    pointer: usize,
) -> Option<Value> {
    use in_type::{
        ANSI_STRING, BINARY, COUNTED_STRING, MANIFEST_COUNTED_BINARY, MANIFEST_COUNTED_STRING, SID,
        UNICODE_STRING, WBEMSID,
    };
    if prop.flags & PROPERTY_STRUCT != 0 {
        return None;
    }
    let count = if prop.flags & PROPERTY_PARAM_COUNT == 0 {
        usize::from(prop.count.max(1))
    } else {
        param(out, prop.count)?
    };
    let length = if prop.flags & PROPERTY_PARAM_LENGTH == 0 {
        usize::from(prop.length)
    } else {
        param(out, prop.length)?
    };
    let rest = data.get(*at..)?;
    if let Some(size) = scalar_size(prop.in_type, pointer) {
        let total = size.checked_mul(count)?;
        let bytes = rest.get(..total)?;
        *at += total;
        return Some(if count == 1 && size <= 8 {
            Value::Int(little_endian(bytes))
        } else {
            Value::Bytes(total)
        });
    }
    if count != 1 {
        return None;
    }
    let (value, used) = match prop.in_type {
        UNICODE_STRING => {
            let units = rest.as_chunks::<2>().0;
            let (chars, used) = if length > 0 {
                (length, length * 2)
            } else {
                // A missing terminator ends the string at the end of the data.
                units
                    .iter()
                    .position(|u| *u == [0, 0])
                    .map_or((units.len(), rest.len()), |n| (n, n * 2 + 2))
            };
            (Value::Str(utf16(units.get(..chars)?)), used)
        }
        ANSI_STRING => {
            let (chars, used) = if length > 0 {
                (length, length)
            } else {
                rest.iter()
                    .position(|&b| b == 0)
                    .map_or((rest.len(), rest.len()), |n| (n, n + 1))
            };
            let text = String::from_utf8_lossy(rest.get(..chars)?).into_owned();
            (Value::Str(text), used)
        }
        BINARY => (Value::Bytes(length), length),
        MANIFEST_COUNTED_STRING | COUNTED_STRING => {
            let bytes = usize::from(u16::from_le_bytes([*rest.first()?, *rest.get(1)?]));
            let units = rest.get(2..2 + bytes)?.as_chunks::<2>().0;
            (Value::Str(utf16(units)), 2 + bytes)
        }
        MANIFEST_COUNTED_BINARY => {
            let bytes = usize::from(u16::from_le_bytes([*rest.first()?, *rest.get(1)?]));
            (Value::Bytes(bytes), 2 + bytes)
        }
        SID => {
            let size = 8 + 4 * usize::from(*rest.get(1)?);
            (Value::Bytes(size), size)
        }
        WBEMSID => {
            let sid = rest.get(2 * pointer..)?;
            let size = 2 * pointer + 8 + 4 * usize::from(*sid.get(1)?);
            (Value::Bytes(size), size)
        }
        _ => return None,
    };
    if used > rest.len() {
        return None;
    }
    *at += used;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::{
        Layout, PROPERTY_PARAM_LENGTH, PROPERTY_STRUCT, Prop, Value, decode,
        in_type::{BINARY, POINTER, SID, UINT16, UINT32, UINT64, UNICODE_STRING},
    };

    fn prop(name: &str, in_type: u16) -> Prop {
        Prop {
            name: name.into(),
            in_type,
            flags: 0,
            count: 1,
            length: 0,
        }
    }

    fn wstr(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    /// Kernel-Registry `SetValueKey` (event 5): strings, then length-prefixed binaries.
    #[test]
    fn set_value_key() {
        let mut captured = prop("CapturedData", BINARY);
        captured.flags = PROPERTY_PARAM_LENGTH;
        captured.length = 6;
        let layout = Layout {
            task: String::new(),
            props: vec![
                prop("KeyObject", POINTER),
                prop("Status", UINT32),
                prop("Type", UINT32),
                prop("DataSize", UINT32),
                prop("KeyName", UNICODE_STRING),
                prop("ValueName", UNICODE_STRING),
                prop("CapturedDataSize", UINT16),
                captured,
                prop("PreviousDataType", UINT32),
            ],
        };
        let mut data = Vec::new();
        data.extend(0xFFFF_8000_1234_5678_u64.to_le_bytes());
        data.extend(0_u32.to_le_bytes());
        data.extend(1_u32.to_le_bytes());
        data.extend(4_u32.to_le_bytes());
        data.extend(wstr(r"\REGISTRY\MACHINE\SOFTWARE\S07Test"));
        data.extend(wstr("v"));
        data.extend(3_u16.to_le_bytes());
        data.extend([b'x', 0, 0]);
        data.extend(7_u32.to_le_bytes());
        let v = decode(&layout, &data, 8);
        assert_eq!(v[0], Some(Value::Int(0xFFFF_8000_1234_5678)));
        assert_eq!(
            v[layout.index("keyname").unwrap_or(99)]
                .as_ref()
                .and_then(Value::text),
            Some(r"\REGISTRY\MACHINE\SOFTWARE\S07Test")
        );
        assert_eq!(v[5], Some(Value::Str("v".into())));
        assert_eq!(v[7], Some(Value::Bytes(3)));
        assert_eq!(v[8], Some(Value::Int(7)));
    }

    /// Kernel-Process `ProcessStart` v3+: a SID between scalars and the image name.
    #[test]
    fn sid_then_string() {
        let layout = Layout {
            task: "ProcessStart".into(),
            props: vec![
                prop("ProcessID", UINT32),
                prop("ProcessSequenceNumber", UINT64),
                prop("ParentProcessID", UINT32),
                prop("MandatoryLabel", SID),
                prop("ImageName", UNICODE_STRING),
            ],
        };
        let mut data = Vec::new();
        data.extend(4242_u32.to_le_bytes());
        data.extend(9_u64.to_le_bytes());
        data.extend(100_u32.to_le_bytes());
        // S-1-16-12288: revision 1, one sub-authority.
        data.extend([1, 1, 0, 0, 0, 0, 0, 16]);
        data.extend(12288_u32.to_le_bytes());
        data.extend(wstr(
            r"\Device\HarddiskVolume3\Windows\System32\msiexec.exe",
        ));
        let v = decode(&layout, &data, 8);
        assert_eq!(v[2], Some(Value::Int(100)));
        assert_eq!(v[3], Some(Value::Bytes(12)));
        assert!(
            v[4].as_ref()
                .and_then(Value::text)
                .is_some_and(|s| s.ends_with("msiexec.exe"))
        );
    }

    #[test]
    fn struct_or_truncation_stops_decoding() {
        let mut nested = prop("Nested", UINT32);
        nested.flags = PROPERTY_STRUCT;
        let layout = Layout {
            task: String::new(),
            props: vec![prop("A", UINT32), nested, prop("B", UINT32)],
        };
        let v = decode(&layout, &[1, 0, 0, 0, 2, 0, 0, 0], 8);
        assert_eq!(v, vec![Some(Value::Int(1)), None, None]);

        let layout = Layout {
            task: String::new(),
            props: vec![prop("A", UINT64), prop("B", UINT32)],
        };
        assert_eq!(decode(&layout, &[1, 0, 0], 8), vec![None, None]);
    }

    /// The last string may lack its terminator.
    #[test]
    fn unterminated_string() {
        let layout = Layout {
            task: String::new(),
            props: vec![prop("S", UNICODE_STRING)],
        };
        let data: Vec<u8> = "ab".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            decode(&layout, &data, 8),
            vec![Some(Value::Str("ab".into()))]
        );
    }
}
