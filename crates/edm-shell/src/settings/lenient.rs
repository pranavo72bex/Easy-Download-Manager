// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Reading parts of the file one at a time, so one odd entry — a proxy type
//! a later EDM wrote, a field left out by hand — costs that entry and not
//! every setting beside it.

use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

thread_local! {
    /// What the readers below left out, by where it was in the file,
    /// for `load_from` to name. Serde gives a field's reader no way to
    /// say so, and reading happens on the caller's thread.
    static DROPPED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn note(what: String) {
    DROPPED.with(|d| {
        let mut d = d.borrow_mut();
        if !d.contains(&what) {
            d.push(what);
        }
    });
}

/// What was left out since the last call, which forgets it.
pub(super) fn take() -> Vec<String> {
    DROPPED.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

/// `merged`, saying under `at` what was left out.
fn read_at<T: Serialize + DeserializeOwned>(base: &T, given: Value, at: &str) -> Option<T> {
    match merged(base, given) {
        Some((read, dropped)) => {
            for key in dropped {
                note(format!("{at}.{key}"));
            }
            Some(read)
        }
        None => {
            note(at.to_string());
            None
        }
    }
}

/// `given` laid over `base` one field at a time: each field that reads
/// is kept, each that does not is left at the base's. The names of
/// those left out come back with it.
pub(super) fn merged<T: Serialize + DeserializeOwned>(
    base: &T,
    given: Value,
) -> Option<(T, Vec<String>)> {
    let Value::Object(mut fields) = serde_json::to_value(base).ok()? else {
        return None;
    };
    let Value::Object(given) = given else {
        return None;
    };
    let mut dropped = Vec::new();
    for (key, value) in given {
        let before = fields.insert(key.clone(), value);
        if serde_json::from_value::<T>(Value::Object(fields.clone())).is_err() {
            match before {
                Some(old) => fields.insert(key.clone(), old),
                None => fields.remove(&key),
            };
            dropped.push(key);
        }
    }
    serde_json::from_value(Value::Object(fields))
        .ok()
        .map(|read| (read, dropped))
}

/// The window's scale as a number, a percentage or an older file's
/// name for it (`TextSizes::read`); anything else is the default.
pub(super) fn text_size<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let said = serde_json::Value::deserialize(d)?;
    Ok(super::TEXT_SIZE
        .read(&said)
        .unwrap_or(super::TEXT_SIZE.default))
}

pub(super) fn schedule<'de, D: Deserializer<'de>>(d: D) -> Result<edm_core::Schedule, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(read_at(&edm_core::Schedule::default(), value, "schedule").unwrap_or_default())
}

/// A proxy whose type this EDM does not know is turned off rather than
/// used as some other kind: its host and port mean nothing without it.
pub(super) fn proxy<'de, D: Deserializer<'de>>(d: D) -> Result<edm_core::Proxy, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(match merged(&edm_core::Proxy::default(), value) {
        Some((mut read, dropped)) => {
            if dropped.iter().any(|k| k == "type") {
                if read.enabled {
                    read.enabled = false;
                    note("proxy.type (so the proxy is off)".into());
                } else {
                    note("proxy.type".into());
                }
            }
            for key in dropped.into_iter().filter(|k| k != "type") {
                note(format!("proxy.{key}"));
            }
            read
        }
        None => {
            note("proxy".into());
            edm_core::Proxy::default()
        }
    })
}

/// A list called `at`, each entry read over `base` and one that is not
/// an entry at all left out, each said.
fn list_of<'de, D, T>(d: D, base: &T, at: &str) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Serialize + DeserializeOwned,
{
    let Value::Array(items) = Value::deserialize(d)? else {
        note(at.to_string());
        return Ok(Vec::new());
    };
    Ok(items
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| read_at(base, item, &format!("{at}[{i}]")))
        .collect())
}

pub(super) fn custom_categories<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<edm_core::Custom>, D::Error> {
    list_of(d, &edm_core::Custom::default(), "custom_categories")
}

pub(super) fn site_logins<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<super::SiteLogin>, D::Error> {
    list_of(d, &super::SiteLogin::default(), "site_logins")
}

pub(super) fn site_rules<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<super::SiteRule>, D::Error> {
    list_of(d, &super::SiteRule::default(), "site_rules")
}

pub(super) fn queue_limits<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<super::QueueLimit>, D::Error> {
    list_of(d, &super::QueueLimit::default(), "queue_limits")
}

/// The same for site limits, whose type has no default of its own: no
/// site (dropped when clamped) and one connection.
pub(super) fn host_limits<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<edm_core::HostLimit>, D::Error> {
    let base = edm_core::HostLimit {
        host: String::new(),
        connections: 1,
    };
    list_of(d, &base, "host_limits")
}

/// The whole file the same way, for when it does not read as it is.
pub(super) fn settings(text: &str) -> Option<(super::Settings, Vec<String>)> {
    let value: Value = serde_json::from_str(text).ok()?;
    let Value::Object(_) = value else {
        return None;
    };
    merged(&super::Settings::default(), value)
}
