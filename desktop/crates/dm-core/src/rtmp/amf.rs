//! The AMF values a stream's commands are made of.

/// The big-endian integer in the first `n` bytes of `b`; zero where `b`
/// is shorter.
pub(super) fn be(b: &[u8], n: usize) -> u32 {
    b.get(..n)
        .map_or(0, |v| v.iter().fold(0u32, |a, x| (a << 8) | u32::from(*x)))
}

// ---- AMF0 ---------------------------------------------------------------

/// An AMF0 value, as much of one as a player reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Amf {
    Number(f64),
    Bool(bool),
    String(String),
    Object(Vec<(String, Amf)>),
    Null,
    Other,
}

impl Amf {
    pub(super) fn field(&self, name: &str) -> Option<&Amf> {
        match self {
            Amf::Object(fields) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(super) fn text(&self) -> Option<&str> {
        match self {
            Amf::String(s) => Some(s),
            _ => None,
        }
    }
}

pub(super) fn amf_string(out: &mut Vec<u8>, s: &str) {
    out.push(0x02);
    out.extend_from_slice(&(s.len().min(0xFFFF) as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes().get(..s.len().min(0xFFFF)).unwrap_or_default());
}

pub(super) fn amf_number(out: &mut Vec<u8>, n: f64) {
    out.push(0x00);
    out.extend_from_slice(&n.to_be_bytes());
}

pub(super) fn amf_object(out: &mut Vec<u8>, fields: &[(&str, Amf)]) {
    out.push(0x03);
    for (key, value) in fields {
        out.extend_from_slice(&(key.len() as u16).to_be_bytes());
        out.extend_from_slice(key.as_bytes());
        match value {
            Amf::Number(n) => amf_number(out, *n),
            Amf::Bool(b) => out.extend_from_slice(&[0x01, u8::from(*b)]),
            Amf::String(s) => amf_string(out, s),
            _ => out.push(0x05),
        }
    }
    out.extend_from_slice(&[0, 0, 0x09]);
}

/// The AMF0 values in `data`, as many as can be read.
pub fn amf_values(data: &[u8]) -> Vec<Amf> {
    let mut at = 0;
    let mut values = Vec::new();
    while at < data.len() {
        match amf_value(data, &mut at, 0) {
            Some(v) => values.push(v),
            None => break,
        }
    }
    values
}

pub(super) fn amf_value(data: &[u8], at: &mut usize, depth: u32) -> Option<Amf> {
    if depth > 16 {
        return None;
    }
    let marker = *data.get(*at)?;
    *at += 1;
    let short_string = |at: &mut usize| -> Option<String> {
        let len = usize::from(u16::from_be_bytes([*data.get(*at)?, *data.get(*at + 1)?]));
        let text = data.get(*at + 2..*at + 2 + len)?;
        *at += 2 + len;
        Some(String::from_utf8_lossy(text).into_owned())
    };
    let fields = |at: &mut usize| -> Option<Vec<(String, Amf)>> {
        let mut fields = Vec::new();
        loop {
            if data.get(*at..*at + 3) == Some(&[0, 0, 0x09]) {
                *at += 3;
                return Some(fields);
            }
            let key = short_string(at)?;
            let value = amf_value(data, at, depth + 1)?;
            fields.push((key, value));
        }
    };
    Some(match marker {
        0x00 => {
            let n = f64::from_be_bytes(data.get(*at..*at + 8)?.try_into().ok()?);
            *at += 8;
            Amf::Number(n)
        }
        0x01 => {
            let b = *data.get(*at)? != 0;
            *at += 1;
            Amf::Bool(b)
        }
        0x02 => Amf::String(short_string(at)?),
        0x03 => Amf::Object(fields(at)?),
        0x05 | 0x06 => Amf::Null,
        0x08 => {
            *at += 4;
            Amf::Object(fields(at)?)
        }
        0x0A => {
            let count = u32::from_be_bytes(data.get(*at..*at + 4)?.try_into().ok()?);
            *at += 4;
            for _ in 0..count.min(4096) {
                amf_value(data, at, depth + 1)?;
            }
            Amf::Other
        }
        0x0B => {
            *at += 10;
            Amf::Other
        }
        0x0C => {
            let len = u32::from_be_bytes(data.get(*at..*at + 4)?.try_into().ok()?) as usize;
            let text = data.get(*at + 4..(*at + 4).checked_add(len)?)?;
            *at += 4 + len;
            Amf::String(String::from_utf8_lossy(text).into_owned())
        }
        _ => return None,
    })
}

// ---- the chunk stream ---------------------------------------------------
