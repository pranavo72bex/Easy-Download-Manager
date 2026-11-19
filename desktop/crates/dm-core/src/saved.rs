// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The format number each JSON file dm saves carries (gaps row A8): `"format": N`, beside what
//! the file holds, so a later dm can tell which shape it is reading. Each file has its own
//! number, from 1, raised when the file's shape changes in a way its fields alone do not say.
//!
//! Read leniently both ways. A file from before the numbers (none written: format 0) reads as it
//! always did. A file with a higher number than this dm knows is read as far as it can be: the
//! fields it does not know are passed over (no saved type refuses an unknown field), so an older
//! dm opened on a newer file loses nothing it understood.
//!
//! A file of one object is written as that object with `format` among its fields
//! ([`Stamped`]). A file that was a bare list (the queue, its secrets, the formats cache) is
//! written as `{"format": N, "rows": [...]}` ([`rows_to_vec`]), and either shape reads back
//! ([`rows_from_slice`]).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// `body`'s fields with the file's format number before them: what a saved file of one object
/// is written as.
#[derive(Serialize)]
pub struct Stamped<'a, T> {
    /// The file's format number.
    pub format: u32,
    /// What the file holds; its fields are written beside `format`.
    #[serde(flatten)]
    pub body: &'a T,
}

/// A list written with its format number.
#[derive(Serialize)]
struct RowsOut<'a, T> {
    format: u32,
    rows: &'a [T],
}

/// A list read with its format number; the number itself, and anything a newer dm put beside
/// the rows, passed over.
#[derive(Deserialize)]
struct RowsIn<T> {
    rows: Vec<T>,
}

/// Just the number, from a file of one object.
#[derive(Deserialize)]
struct FormatOnly {
    #[serde(default)]
    format: u32,
}

/// Whether `json` is an object (rather than a list), by its first character that is not space.
fn is_object(json: &[u8]) -> bool {
    json.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{')
}

/// `rows` as a saved list of format `format`: `{"format": N, "rows": [...]}`, indented where
/// `pretty`.
pub fn rows_to_vec<T: Serialize>(
    format: u32,
    rows: &[T],
    pretty: bool,
) -> serde_json::Result<Vec<u8>> {
    let file = RowsOut { format, rows };
    if pretty {
        serde_json::to_vec_pretty(&file)
    } else {
        serde_json::to_vec(&file)
    }
}

/// The rows of a saved list, written either way: as [`rows_to_vec`] writes it, of any format,
/// or as a bare list, as dm wrote it before the numbers. The error is serde's for the shape
/// found, so a file that is neither says why as before.
pub fn rows_from_slice<T: DeserializeOwned>(json: &[u8]) -> serde_json::Result<Vec<T>> {
    if is_object(json) {
        serde_json::from_slice::<RowsIn<T>>(json).map(|file| file.rows)
    } else {
        serde_json::from_slice(json)
    }
}

/// The format number a saved file says it is in: 0 where it says none (a file from before the
/// numbers, a bare list, or not JSON).
pub fn format_of(json: &[u8]) -> u32 {
    if !is_object(json) {
        return 0;
    }
    serde_json::from_slice::<FormatOnly>(json).map_or(0, |file| file.format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
    struct Thing {
        name: String,
        #[serde(default)]
        size: u64,
    }

    #[test]
    fn a_list_is_written_with_its_number_and_read_back_either_way() {
        let rows = vec![
            Thing {
                name: "a".into(),
                size: 1,
            },
            Thing {
                name: "b".into(),
                size: 2,
            },
        ];
        for pretty in [true, false] {
            let bytes = rows_to_vec(1, &rows, pretty).unwrap();
            assert_eq!(format_of(&bytes), 1);
            assert_eq!(rows_from_slice::<Thing>(&bytes).unwrap(), rows);
        }
        // Before the numbers: a bare list, format 0, read as ever.
        let bare = serde_json::to_vec(&rows).unwrap();
        assert_eq!(format_of(&bare), 0);
        assert_eq!(rows_from_slice::<Thing>(&bare).unwrap(), rows);
        // A newer dm's: a higher number and a field beside the rows, read as far as it goes.
        let newer = br#" {"format": 7, "rows": [{"name": "a", "size": 1, "colour": "red"}],
            "more": true}"#;
        assert_eq!(format_of(newer), 7);
        assert_eq!(rows_from_slice::<Thing>(newer).unwrap(), &rows[..1]);
        assert!(rows_from_slice::<Thing>(b"{\"format\": 1}").is_err());
        assert!(rows_from_slice::<Thing>(b"not json").is_err());
    }

    #[test]
    fn an_object_is_written_with_its_number_among_its_fields() {
        let thing = Thing {
            name: "a".into(),
            size: 3,
        };
        let text = serde_json::to_string(&Stamped {
            format: 2,
            body: &thing,
        })
        .unwrap();
        assert_eq!(text, r#"{"format":2,"name":"a","size":3}"#);
        assert_eq!(format_of(text.as_bytes()), 2);
        assert_eq!(serde_json::from_str::<Thing>(&text).unwrap(), thing);
        assert_eq!(format_of(br#"{"name": "a"}"#), 0);
    }
}
