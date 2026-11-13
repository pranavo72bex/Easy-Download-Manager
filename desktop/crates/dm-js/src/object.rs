// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An object: its properties in the order the language gives them (integer keys first, then
//! the rest as they were added), its prototype, and what a built-in object carries inside (an
//! array's elements, a function's code, a map's entries).
//!
//! Objects are shared (`Rc`) and changed in place (`RefCell`). A cycle of objects is not freed
//! until the interpreter that made them is dropped, which is at the end of each run: dm runs a
//! program once and is done with it.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast;
use crate::lexer::Text;
use crate::value::{Key, Value};

/// A shared object.
#[derive(Clone)]
pub struct Obj(pub Rc<RefCell<Object>>);

impl std::fmt::Debug for Obj {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[object {:p}]", Rc::as_ptr(&self.0))
    }
}

impl Obj {
    /// A new object with `proto` and inner `class`.
    pub fn new(proto: Option<Obj>, class: Class) -> Obj {
        Obj(Rc::new(RefCell::new(Object {
            proto,
            props: Props::default(),
            class,
            extensible: true,
        })))
    }

    /// The object, to read.
    pub fn borrow(&self) -> Ref<'_, Object> {
        self.0.borrow()
    }

    /// The object, to change.
    pub fn borrow_mut(&self) -> RefMut<'_, Object> {
        self.0.borrow_mut()
    }

    /// The same object as `other`.
    pub fn same(&self, other: &Obj) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Whether it can be called.
    pub fn is_callable(&self) -> bool {
        matches!(self.borrow().class, Class::Function(_))
    }

    /// Its prototype.
    pub fn proto(&self) -> Option<Obj> {
        self.borrow().proto.clone()
    }

    /// Its own property `key`, plainly.
    pub fn own(&self, key: &Key) -> Option<Prop> {
        let me = self.borrow();
        if let Class::Array(items) = &me.class {
            if let Some(i) = key.as_index() {
                return items
                    .get(i)
                    .map(|v| Prop::data(v.clone(), true, true, true));
            }
            if *key == Key::str("length") {
                return Some(Prop::data(
                    Value::Number(items.len() as f64),
                    true,
                    false,
                    false,
                ));
            }
        }
        if let Class::String(s) = &me.class {
            if let Some(i) = key.as_index() {
                return s
                    .get(i)
                    .map(|c| Prop::data(Value::String(vec![*c].into()), false, true, false));
            }
            if *key == Key::str("length") {
                return Some(Prop::data(
                    Value::Number(s.len() as f64),
                    false,
                    false,
                    false,
                ));
            }
        }
        if let Class::Bytes(b) = &me.class {
            if (1..10).contains(&b.kind) {
                if let Some(i) = key.as_index() {
                    return b
                        .get(i)
                        .map(|n| Prop::data(Value::Number(n), true, true, false));
                }
            }
        }
        me.props.get(key).cloned()
    }

    /// Sets an own data property, writable, enumerable and configurable (as `o.k = v` makes
    /// one).
    pub fn set_own(&self, key: Key, value: Value) {
        self.define(key, Prop::data(value, true, true, true));
    }

    /// Sets an own data property that `for … in` and `Object.keys` do not list (as the built-in
    /// objects' methods are).
    pub fn set_hidden(&self, key: Key, value: Value) {
        self.define(key, Prop::data(value, true, false, true));
    }

    /// Defines an own property as given.
    pub fn define(&self, key: Key, prop: Prop) {
        let mut me = self.borrow_mut();
        if let Class::Array(items) = &mut me.class {
            if let Some(i) = key.as_index() {
                if let Slot::Data(value) = &prop.slot {
                    let value = value.clone();
                    if i < items.len() {
                        items[i] = value;
                        return;
                    }
                    // A little past the end grows the array; far past it, holes are filled with
                    // undefined (dm meets no sparse arrays it must keep sparse).
                    if i <= items.len() + 1_000_000 {
                        items.resize(i, Value::Undefined);
                        items.push(value);
                        return;
                    }
                }
            }
            if key == Key::str("length") {
                if let Slot::Data(Value::Number(n)) = prop.slot {
                    items.resize(n.max(0.0) as usize, Value::Undefined);
                    return;
                }
            }
        }
        if let Class::Bytes(b) = &me.class {
            if (1..10).contains(&b.kind) {
                if let Some(i) = key.as_index() {
                    if let Slot::Data(Value::Number(n)) = &prop.slot {
                        b.set(i, *n);
                    }
                    return;
                }
            }
        }
        me.props.insert(key, prop);
    }

    /// Deletes an own property; whether it is gone.
    pub fn delete(&self, key: &Key) -> bool {
        let mut me = self.borrow_mut();
        if let Class::Array(items) = &mut me.class {
            if let Some(i) = key.as_index() {
                if i + 1 == items.len() {
                    // The last one: undefined there; the length stays, as the language keeps it.
                    items[i] = Value::Undefined;
                } else if i < items.len() {
                    items[i] = Value::Undefined;
                }
                return true;
            }
        }
        match me.props.get(key) {
            Some(p) if !p.configurable => false,
            Some(_) => {
                me.props.remove(key);
                true
            }
            None => true,
        }
    }

    /// Its own keys in the language's order: integer keys ascending, then strings as added,
    /// then symbols as added. Only the enumerable ones where `enumerable_only`.
    pub fn own_keys(&self, enumerable_only: bool) -> Vec<Key> {
        let me = self.borrow();
        let mut ints: Vec<(usize, Key)> = Vec::new();
        let mut strings = Vec::new();
        let mut symbols = Vec::new();
        match &me.class {
            Class::Array(items) => {
                ints.extend((0..items.len()).map(|i| (i, Key::index(i))));
                if !enumerable_only {
                    strings.push(Key::str("length"));
                }
            }
            Class::Bytes(b) if (1..10).contains(&b.kind) => {
                ints.extend((0..b.length).map(|i| (i, Key::index(i))));
            }
            Class::String(s) => {
                ints.extend((0..s.len()).map(|i| (i, Key::index(i))));
                if !enumerable_only {
                    strings.push(Key::str("length"));
                }
            }
            _ => {}
        }
        for (key, prop) in me.props.iter() {
            if enumerable_only && !prop.enumerable {
                continue;
            }
            match key {
                Key::Symbol(_) => symbols.push(key.clone()),
                Key::String(_) => match key.as_index() {
                    Some(i) => ints.push((i, key.clone())),
                    None => strings.push(key.clone()),
                },
            }
        }
        ints.sort_by_key(|(i, _)| *i);
        let mut out: Vec<Key> = ints.into_iter().map(|(_, k)| k).collect();
        out.extend(strings);
        out.extend(symbols);
        out
    }
}

/// An object's data.
pub struct Object {
    /// Its prototype.
    pub proto: Option<Obj>,
    /// Its own properties (an array's elements and a string's characters are in `class`).
    pub props: Props,
    /// What it is inside.
    pub class: Class,
    /// Properties can be added (`Object.preventExtensions` makes it not).
    pub extensible: bool,
}

/// What an object is inside.
pub enum Class {
    /// A plain object.
    Ordinary,
    /// An array's elements (holes read as undefined).
    Array(Vec<Value>),
    /// A function.
    Function(Func),
    /// An error (its `stack` is not kept).
    Error,
    /// `new Boolean(…)`
    Boolean(bool),
    /// `new Number(…)`
    Number(f64),
    /// `new String(…)`
    String(Text),
    /// `Object(symbol)`
    Symbol(Rc<crate::value::Symbol>),
    /// `Object(bigint)`
    BigInt(i128),
    /// A date: milliseconds since 1970 (what `new Date()` reads is the clock, which a program
    /// here does not have: it is 0).
    Date(f64),
    /// A pattern: its source, flags, compiled form, and `lastIndex` in the properties.
    RegExp(Rc<crate::regexp::Regex>),
    /// A `Map`'s entries, in the order they were set.
    Map(Entries),
    /// A `Set`'s values, in the order they were added.
    Set(Entries),
    /// A `WeakMap` (held strongly: nothing is collected while a program runs).
    WeakMap(Entries),
    /// A `WeakSet`.
    WeakSet(Entries),
    /// A function's `arguments`.
    Arguments,
    /// An iterator the built-in objects make: what it walks and where it is.
    Iterator(IterState),
    /// A promise: its state, and what runs when it settles.
    Promise(crate::promise::PromiseState),
    /// An `ArrayBuffer`'s or a typed array's bytes.
    Bytes(crate::typed::Bytes),
    /// A `Proxy`'s target and handler.
    Proxy(Obj, Obj),
}

/// A `Map`'s or `Set`'s entries, in order, a deleted one left as `None` so iterators keep their
/// place.
#[derive(Default)]
pub struct Entries {
    /// Each entry, a key and a value (a `Set`'s value is its key).
    pub list: Vec<Option<(Value, Value)>>,
}

impl Entries {
    /// Where `key` is.
    pub fn find(&self, key: &Value) -> Option<usize> {
        self.list
            .iter()
            .position(|e| e.as_ref().is_some_and(|(k, _)| k.same_value_zero(key)))
    }

    /// How many entries there are.
    pub fn size(&self) -> usize {
        self.list.iter().filter(|e| e.is_some()).count()
    }
}

/// What a built-in iterator walks.
pub enum IterState {
    /// An array's (or array-like's) keys, values or entries.
    Array {
        /// What is walked.
        of: Value,
        /// The next index.
        at: usize,
        /// 0 keys, 1 values, 2 entries.
        kind: u8,
        /// Walked to its end.
        done: bool,
    },
    /// A string's characters (whole code points).
    String {
        /// The string.
        of: Text,
        /// The next unit.
        at: usize,
    },
    /// A `Map`'s or `Set`'s entries.
    Entries {
        /// The map or set.
        of: Obj,
        /// The next entry.
        at: usize,
        /// 0 keys, 1 values, 2 entries.
        kind: u8,
    },
    /// A pattern's matches (`matchAll`).
    Matches {
        /// The pattern.
        pattern: Obj,
        /// The string.
        of: Text,
        /// Walked to its end.
        done: bool,
    },
}

/// A function: written in the program, built in, or bound.
pub enum Func {
    /// Written in the program.
    Script(Rc<ScriptFunc>),
    /// Built in: the Rust that runs it.
    Native(Native),
    /// `f.bind(this, …args)`
    Bound {
        /// The function bound.
        target: Obj,
        /// Its `this`.
        this: Value,
        /// The arguments put first.
        args: Vec<Value>,
    },
}

/// A built-in function's code: given the interpreter, `this`, the arguments, and the object
/// `new` made (none where it is called plainly).
pub type NativeFn =
    fn(&mut crate::interp::Interp, &Value, &[Value], Option<&Obj>) -> crate::interp::Res<Value>;

/// A built-in function.
#[derive(Clone)]
pub struct Native {
    /// The code.
    pub run: NativeFn,
    /// Whether `new` can make an object with it.
    pub constructor: bool,
    /// What it carries (a promise's resolve function carries its promise).
    pub data: Option<Value>,
}

/// A function written in the program, with the scope it was made in.
pub struct ScriptFunc {
    /// Its code.
    pub code: Rc<ast::Function>,
    /// The scope it closes over.
    pub scope: crate::interp::Scope,
    /// An arrow's `this` (the surrounding function's).
    pub this: Option<Value>,
    /// A method's object, where `super` looks.
    pub home: Option<Obj>,
    /// A class's constructor: its class (for its fields), and whether it extends another.
    pub class: Option<ClassInfo>,
}

/// What a class's constructor knows of its class.
pub struct ClassInfo {
    /// The class's code.
    pub code: Rc<ast::Class>,
    /// It extends another (its `this` comes from `super()`).
    pub derived: bool,
    /// The scope its fields' values are worked out in.
    pub scope: crate::interp::Scope,
    /// The class's prototype (for its fields' methods' `super`).
    pub prototype: Obj,
}

/// A property: a value or a getter and setter, and its flags.
#[derive(Clone)]
pub struct Prop {
    /// What it holds.
    pub slot: Slot,
    /// `for … in` and `Object.keys` list it.
    pub enumerable: bool,
    /// It can be deleted and redefined.
    pub configurable: bool,
}

/// A property's value, or its getter and setter.
#[derive(Clone)]
pub enum Slot {
    /// A value, and whether it can be changed.
    Data(Value),
    /// A data property that cannot be changed.
    ReadOnly(Value),
    /// A getter and a setter.
    Accessor(Option<Obj>, Option<Obj>),
}

impl Prop {
    /// A data property.
    pub fn data(value: Value, writable: bool, enumerable: bool, configurable: bool) -> Prop {
        Prop {
            slot: if writable {
                Slot::Data(value)
            } else {
                Slot::ReadOnly(value)
            },
            enumerable,
            configurable,
        }
    }

    /// Whether its value can be changed.
    pub fn writable(&self) -> bool {
        matches!(self.slot, Slot::Data(_))
    }
}

/// Properties in the order they were added, found by key.
#[derive(Default)]
pub struct Props {
    order: Vec<Option<(Key, Prop)>>,
    at: HashMap<Key, usize>,
    removed: usize,
}

impl Props {
    /// The property `key`.
    pub fn get(&self, key: &Key) -> Option<&Prop> {
        let i = *self.at.get(key)?;
        self.order[i].as_ref().map(|(_, p)| p)
    }

    /// Sets `key` (in its place where it is there, else last).
    pub fn insert(&mut self, key: Key, prop: Prop) {
        if let Some(&i) = self.at.get(&key) {
            if let Some((_, p)) = self.order[i].as_mut() {
                *p = prop;
                return;
            }
        }
        self.at.insert(key.clone(), self.order.len());
        self.order.push(Some((key, prop)));
    }

    /// Removes `key`.
    pub fn remove(&mut self, key: &Key) {
        if let Some(i) = self.at.remove(key) {
            self.order[i] = None;
            self.removed += 1;
            // Room taken back once most of the list is gone.
            if self.removed > 32 && self.removed * 2 > self.order.len() {
                let kept: Vec<_> = self.order.drain(..).flatten().collect();
                self.at.clear();
                self.removed = 0;
                for (key, prop) in kept {
                    self.at.insert(key.clone(), self.order.len());
                    self.order.push(Some((key, prop)));
                }
            }
        }
    }

    /// Every property, in the order added.
    pub fn iter(&self) -> impl Iterator<Item = (&Key, &Prop)> {
        self.order.iter().flatten().map(|(k, p)| (k, p))
    }
}
