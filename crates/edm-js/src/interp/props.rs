// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Reading and writing properties: `in`, getting along the prototypes (and a primitive's), setting
//! through setters, and a `for … in`'s keys.

use super::*;

impl Interp {
    /// A plain object.
    pub fn plain_object(&self) -> Obj {
        Obj::new(Some(self.intrinsics.object_proto.clone()), Class::Ordinary)
    }

    /// Whether `o` or its prototypes have `key`.
    pub fn has_property(&self, o: &Obj, key: &Key) -> bool {
        let mut at = Some(o.clone());
        while let Some(o) = at {
            if o.own(key).is_some() {
                return true;
            }
            at = o.proto();
        }
        false
    }

    /// `value[key]`, a primitive's from its prototype.
    pub fn get_of(&mut self, value: &Value, key: &Key) -> Res<Value> {
        match value {
            Value::Object(_) => self.get(value, key),
            Value::Undefined | Value::Null => {
                let what = String::from_utf16_lossy(&self.key_text(key));
                Err(self.type_error(&format!(
                    "Cannot read properties of {} (reading '{what}')",
                    if matches!(value, Value::Null) {
                        "null"
                    } else {
                        "undefined"
                    }
                )))
            }
            Value::String(s) => {
                if let Some(i) = key.as_index() {
                    return Ok(s
                        .get(i)
                        .map(|c| Value::String(vec![*c].into()))
                        .unwrap_or(Value::Undefined));
                }
                if *key == Key::str("length") {
                    return Ok(Value::Number(s.len() as f64));
                }
                let proto = self.intrinsics.string_proto.clone();
                self.get_from(&proto, key, value)
            }
            other => {
                let proto = self.proto_of_primitive(other);
                self.get_from(&proto, key, value)
            }
        }
    }

    /// The prototype a primitive's methods are on.
    pub fn proto_of_primitive(&self, v: &Value) -> Obj {
        match v {
            Value::Number(_) => self.intrinsics.number_proto.clone(),
            Value::Bool(_) => self.intrinsics.boolean_proto.clone(),
            Value::String(_) => self.intrinsics.string_proto.clone(),
            Value::Symbol(_) => self.intrinsics.symbol_proto.clone(),
            Value::BigInt(_) => self.intrinsics.bigint_proto.clone(),
            _ => self.intrinsics.object_proto.clone(),
        }
    }

    /// `o[key]` (an object's).
    pub fn get(&mut self, o: &Value, key: &Key) -> Res<Value> {
        match o {
            Value::Object(obj) => {
                if let Class::Proxy(target, handler) = &obj.borrow().class {
                    let (target, handler) = (target.clone(), handler.clone());
                    return crate::builtins::proxy::get(self, &target, &handler, key, o);
                }
                let obj = obj.clone();
                self.get_from(&obj, key, o)
            }
            other => self.get_of(other, key),
        }
    }

    /// `key` looked up from `start` along its prototypes, a getter called on `receiver`.
    pub fn get_from(&mut self, start: &Obj, key: &Key, receiver: &Value) -> Res<Value> {
        let mut at = Some(start.clone());
        while let Some(o) = at {
            if let Some(p) = o.own(key) {
                return match p.slot {
                    Slot::Data(v) | Slot::ReadOnly(v) => Ok(v),
                    Slot::Accessor(Some(getter), _) => {
                        self.call(&Value::Object(getter), receiver.clone(), &[])
                    }
                    Slot::Accessor(None, _) => Ok(Value::Undefined),
                };
            }
            at = o.proto();
        }
        Ok(Value::Undefined)
    }

    /// `o[key] = value`: a setter along the prototypes is called; a value is set on `o`.
    pub fn set(&mut self, o: &Value, key: Key, value: Value) -> Res<()> {
        let obj = match o {
            Value::Object(obj) => obj.clone(),
            // Set on a primitive: lost, as sloppy code loses it.
            _ => return Ok(()),
        };
        if let Class::Proxy(target, handler) = &obj.borrow().class {
            let (target, handler) = (target.clone(), handler.clone());
            return crate::builtins::proxy::set(self, &target, &handler, key, value, o);
        }
        // A typed array's element: the value as a number.
        let typed = matches!(&obj.borrow().class, Class::Bytes(b) if (1..10).contains(&b.kind));
        if typed && key.as_index().is_some() {
            let n = self.to_number(&value)?;
            obj.define(key, Prop::data(Value::Number(n), true, true, false));
            return Ok(());
        }
        // Own data property: changed in place, the usual case.
        if let Some(p) = obj.own(&key) {
            match p.slot {
                Slot::Data(_) => {
                    obj.define(
                        key,
                        Prop {
                            slot: Slot::Data(value),
                            enumerable: p.enumerable,
                            configurable: p.configurable,
                        },
                    );
                    return Ok(());
                }
                Slot::ReadOnly(_) => return Ok(()),
                Slot::Accessor(Some(setter), _) => {
                    self.call(&Value::Object(setter), o.clone(), &[value])?;
                    return Ok(());
                }
                Slot::Accessor(None, _) => return Ok(()),
            }
        }
        let mut at = obj.proto();
        while let Some(p) = at {
            if let Some(prop) = p.own(&key) {
                match prop.slot {
                    Slot::Accessor(Some(setter), _) => {
                        self.call(&Value::Object(setter), o.clone(), &[value])?;
                        return Ok(());
                    }
                    Slot::Accessor(None, _) | Slot::ReadOnly(_) => return Ok(()),
                    Slot::Data(_) => break,
                }
            }
            at = p.proto();
        }
        if !obj.borrow().extensible {
            return Ok(());
        }
        obj.set_own(key, value);
        Ok(())
    }

    /// The keys `for … in` walks: enumerable string keys, own first, then the prototypes',
    /// each once.
    pub(super) fn for_in_keys(&mut self, v: &Value) -> Res<Vec<Value>> {
        let start = match v {
            Value::Undefined | Value::Null => return Ok(Vec::new()),
            Value::Object(o) => o.clone(),
            other => self.to_object(other)?,
        };
        let mut seen: Vec<Key> = Vec::new();
        let mut out = Vec::new();
        let mut at = Some(start);
        while let Some(o) = at {
            for key in o.own_keys(false) {
                if matches!(key, Key::Symbol(_)) || seen.contains(&key) {
                    continue;
                }
                let enumerable = o.own(&key).is_some_and(|p| p.enumerable);
                seen.push(key.clone());
                if enumerable {
                    out.push(key.to_value());
                }
            }
            at = o.proto();
        }
        Ok(out)
    }
}
