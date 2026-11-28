// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Protocol Buffers' wire format, as much of it as YouTube's framed stream needs: varints,
//! length-delimited fields and fixed 32- and 64-bit ones, read and written by field number.
//! EDM's own, of no schema: a message is read by asking for a field's number.

/// A field's value as the wire carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    /// Wire type 0: a number of up to 64 bits, seven to a byte.
    Varint(u64),
    /// Wire type 1: eight bytes, little-endian.
    Fixed64(u64),
    /// Wire type 2: bytes, a string or a message inside.
    Bytes(&'a [u8]),
    /// Wire type 5: four bytes, little-endian.
    Fixed32(u32),
}

/// The varint at the start of `data`: its value and how many bytes it took. `None` where it
/// runs past the end, or past the ten bytes a 64-bit number takes at most.
pub fn varint(data: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, byte) in data.iter().take(10).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// `value` written as a varint at the end of `into`.
pub fn put_varint(into: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        into.push((value & 0x7f) as u8 | 0x80);
        value >>= 7;
    }
    into.push(value as u8);
}

/// The field at the start of `data`: its number, its value and how many bytes it took. `None`
/// where it is cut off, numbered 0, or of a wire type EDM does not read (the old groups, 3
/// and 4).
fn field(data: &[u8]) -> Option<(u32, Value<'_>, usize)> {
    let (key, at) = varint(data)?;
    let number = u32::try_from(key >> 3).ok().filter(|n| *n > 0)?;
    let rest = data.get(at..)?;
    let (value, used) = match key & 7 {
        0 => {
            let (value, used) = varint(rest)?;
            (Value::Varint(value), used)
        }
        1 => {
            let bytes: [u8; 8] = rest.get(..8)?.try_into().ok()?;
            (Value::Fixed64(u64::from_le_bytes(bytes)), 8)
        }
        2 => {
            let (len, used) = varint(rest)?;
            let end = used.checked_add(usize::try_from(len).ok()?)?;
            (Value::Bytes(rest.get(used..end)?), end)
        }
        5 => {
            let bytes: [u8; 4] = rest.get(..4)?.try_into().ok()?;
            (Value::Fixed32(u32::from_le_bytes(bytes)), 4)
        }
        _ => return None,
    };
    Some((number, value, at + used))
}

/// Every field of a message in order, by number. Ends at the first that cannot be read
/// ([`Fields::broken`] then says so).
#[derive(Debug, Clone)]
pub struct Fields<'a> {
    rest: &'a [u8],
    broken: bool,
}

impl Fields<'_> {
    /// Whether the fields ended at one that could not be read, rather than at the message's end.
    pub fn broken(&self) -> bool {
        self.broken
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = (u32, Value<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() || self.broken {
            return None;
        }
        match field(self.rest).and_then(|(n, v, used)| Some((n, v, self.rest.get(used..)?))) {
            Some((number, value, rest)) => {
                self.rest = rest;
                Some((number, value))
            }
            None => {
                self.broken = true;
                None
            }
        }
    }
}

/// A message, read by field number. Where a field comes more than once, a number or a string
/// asked for is the last one, as Protocol Buffers' own readers take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message<'a>(pub &'a [u8]);

impl<'a> Message<'a> {
    /// Its fields in order.
    pub fn fields(&self) -> Fields<'a> {
        Fields {
            rest: self.0,
            broken: false,
        }
    }

    /// Whether every byte of it reads as fields.
    pub fn whole(&self) -> bool {
        let mut fields = self.fields();
        let _ = fields.by_ref().count();
        !fields.broken()
    }

    /// Field `number` as an unsigned number (a varint, or a fixed one).
    pub fn uint(&self, number: u32) -> Option<u64> {
        self.fields()
            .filter(|(n, _)| *n == number)
            .filter_map(|(_, value)| match value {
                Value::Varint(v) | Value::Fixed64(v) => Some(v),
                Value::Fixed32(v) => Some(u64::from(v)),
                Value::Bytes(_) => None,
            })
            .last()
    }

    /// Field `number` as a signed number written as a varint (`int32`, `int64`): a negative one
    /// is its two's complement.
    pub fn int(&self, number: u32) -> Option<i64> {
        self.uint(number).map(|v| v as i64)
    }

    /// Field `number` as true or false.
    pub fn flag(&self, number: u32) -> Option<bool> {
        self.uint(number).map(|v| v != 0)
    }

    /// Field `number`'s bytes.
    pub fn bytes(&self, number: u32) -> Option<&'a [u8]> {
        self.all_bytes(number).last()
    }

    /// Every value of field `number` that is bytes, in order (a repeated field).
    pub fn all_bytes(&self, number: u32) -> impl Iterator<Item = &'a [u8]> {
        self.fields()
            .filter(move |(n, _)| *n == number)
            .filter_map(|(_, value)| match value {
                Value::Bytes(bytes) => Some(bytes),
                _ => None,
            })
    }

    /// Field `number` as text, where it is UTF-8.
    pub fn text(&self, number: u32) -> Option<&'a str> {
        std::str::from_utf8(self.bytes(number)?).ok()
    }

    /// Field `number` as a message inside this one.
    pub fn message(&self, number: u32) -> Option<Message<'a>> {
        self.bytes(number).map(Message)
    }
}

/// A message written field by field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    /// An empty message.
    pub fn new() -> Writer {
        Writer::default()
    }

    fn key(&mut self, number: u32, wire: u64) {
        put_varint(&mut self.bytes, (u64::from(number) << 3) | wire);
    }

    /// Field `number`, an unsigned number, as a varint.
    pub fn uint(&mut self, number: u32, value: u64) -> &mut Writer {
        self.key(number, 0);
        put_varint(&mut self.bytes, value);
        self
    }

    /// Field `number`, a signed number, as a varint (`int32`, `int64`).
    pub fn int(&mut self, number: u32, value: i64) -> &mut Writer {
        self.uint(number, value as u64)
    }

    /// Field `number`, true or false.
    pub fn flag(&mut self, number: u32, value: bool) -> &mut Writer {
        self.uint(number, u64::from(value))
    }

    /// Field `number`, eight bytes.
    pub fn fixed64(&mut self, number: u32, value: u64) -> &mut Writer {
        self.key(number, 1);
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// Field `number`, four bytes.
    pub fn fixed32(&mut self, number: u32, value: u32) -> &mut Writer {
        self.key(number, 5);
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// Field `number`, bytes.
    pub fn bytes(&mut self, number: u32, value: &[u8]) -> &mut Writer {
        self.key(number, 2);
        put_varint(&mut self.bytes, value.len() as u64);
        self.bytes.extend_from_slice(value);
        self
    }

    /// Field `number`, text.
    pub fn text(&mut self, number: u32, value: &str) -> &mut Writer {
        self.bytes(number, value.as_bytes())
    }

    /// Field `number`, a message inside this one, written by `build`.
    pub fn message(&mut self, number: u32, build: impl FnOnce(&mut Writer)) -> &mut Writer {
        let mut inner = Writer::new();
        build(&mut inner);
        self.bytes(number, &inner.bytes)
    }

    /// The message's bytes.
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    /// The message's bytes so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}
