// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The world a program starts in: the global object, the built-in prototypes and the
//! well-known symbols ([`make`]), then their methods and constructors ([`fill`], each family in
//! `builtins`). Nothing of the computer is here: no files, programs, network, clock or page.

use std::rc::Rc;

use crate::lexer::text;
use crate::object::{Class, Func, Native, Obj};
use crate::value::{Symbol, Value};

/// The built-in prototypes and symbols the interpreter reaches for itself.
pub struct Intrinsics {
    /// `Object.prototype`
    pub object_proto: Obj,
    /// `Function.prototype`
    pub function_proto: Obj,
    /// `Array.prototype`
    pub array_proto: Obj,
    /// `String.prototype`
    pub string_proto: Obj,
    /// `Number.prototype`
    pub number_proto: Obj,
    /// `Boolean.prototype`
    pub boolean_proto: Obj,
    /// `Symbol.prototype`
    pub symbol_proto: Obj,
    /// `BigInt.prototype`
    pub bigint_proto: Obj,
    /// `Error.prototype`
    pub error_proto: Obj,
    /// `TypeError.prototype`
    pub type_error_proto: Obj,
    /// `RangeError.prototype`
    pub range_error_proto: Obj,
    /// `ReferenceError.prototype`
    pub reference_error_proto: Obj,
    /// `SyntaxError.prototype`
    pub syntax_error_proto: Obj,
    /// `RegExp.prototype`
    pub regexp_proto: Obj,
    /// `Map.prototype`
    pub map_proto: Obj,
    /// `Set.prototype`
    pub set_proto: Obj,
    /// `WeakMap.prototype`
    pub weakmap_proto: Obj,
    /// `WeakSet.prototype`
    pub weakset_proto: Obj,
    /// `Promise.prototype`
    pub promise_proto: Obj,
    /// `Date.prototype`
    pub date_proto: Obj,
    /// `%IteratorPrototype%`
    pub iterator_proto: Obj,
    /// `%ArrayIteratorPrototype%` (strings', maps' and sets' iterators share its `next`)
    pub array_iterator_proto: Obj,
    /// A generator function's `prototype`'s prototype.
    pub generator_proto: Obj,
    /// `ArrayBuffer.prototype`
    pub array_buffer_proto: Obj,
    /// `%TypedArray%.prototype`, each kind's prototype by its number (1 Int8 … 9 Float64).
    pub typed_protos: Vec<Obj>,
    /// `Array.prototype.values` (an array's own iterator, read directly when it is still it).
    pub array_values: Value,
    /// `String.prototype[Symbol.iterator]`
    pub string_iterator: Value,
    /// `Symbol.iterator`
    pub sym_iterator: Rc<Symbol>,
    /// `Symbol.asyncIterator`
    pub sym_async_iterator: Rc<Symbol>,
    /// `Symbol.toPrimitive`
    pub sym_to_primitive: Rc<Symbol>,
    /// `Symbol.hasInstance`
    pub sym_has_instance: Rc<Symbol>,
    /// `Symbol.toStringTag`
    pub sym_to_string_tag: Rc<Symbol>,
    /// `Symbol.species`
    pub sym_species: Rc<Symbol>,
    /// `Symbol.isConcatSpreadable`
    pub sym_concat_spreadable: Rc<Symbol>,
    /// `Symbol.unscopables`
    pub sym_unscopables: Rc<Symbol>,
    /// `Symbol.match`, `matchAll`, `replace`, `search`, `split`
    pub sym_match: Rc<Symbol>,
    /// `Symbol.matchAll`
    pub sym_match_all: Rc<Symbol>,
    /// `Symbol.replace`
    pub sym_replace: Rc<Symbol>,
    /// `Symbol.search`
    pub sym_search: Rc<Symbol>,
    /// `Symbol.split`
    pub sym_split: Rc<Symbol>,
    /// Symbols `Symbol.for(key)` gave, by key.
    pub registry: Vec<(crate::lexer::Text, Rc<Symbol>)>,
}

fn symbol(description: &str) -> Rc<Symbol> {
    Rc::new(Symbol {
        description: Some(text(description)),
    })
}

/// The global object and the bare prototypes, before their methods.
pub fn make() -> (Obj, Intrinsics) {
    let object_proto = Obj::new(None, Class::Ordinary);
    let ordinary = || Obj::new(Some(object_proto.clone()), Class::Ordinary);
    // `Function.prototype` is a function itself, which does nothing.
    let function_proto = Obj::new(
        Some(object_proto.clone()),
        Class::Function(Func::Native(Native {
            run: |_, _, _, _| Ok(Value::Undefined),
            constructor: false,
            data: None,
        })),
    );
    let error_proto = ordinary();
    let error_kind = || Obj::new(Some(error_proto.clone()), Class::Ordinary);
    let iterator_proto = ordinary();
    let typed_base = ordinary();
    let intrinsics = Intrinsics {
        array_proto: Obj::new(Some(object_proto.clone()), Class::Array(Vec::new())),
        string_proto: Obj::new(Some(object_proto.clone()), Class::String(text(""))),
        number_proto: Obj::new(Some(object_proto.clone()), Class::Number(0.0)),
        boolean_proto: Obj::new(Some(object_proto.clone()), Class::Boolean(false)),
        symbol_proto: ordinary(),
        bigint_proto: ordinary(),
        type_error_proto: error_kind(),
        range_error_proto: error_kind(),
        reference_error_proto: error_kind(),
        syntax_error_proto: error_kind(),
        regexp_proto: ordinary(),
        map_proto: ordinary(),
        set_proto: ordinary(),
        weakmap_proto: ordinary(),
        weakset_proto: ordinary(),
        promise_proto: ordinary(),
        date_proto: ordinary(),
        array_iterator_proto: Obj::new(Some(iterator_proto.clone()), Class::Ordinary),
        generator_proto: Obj::new(Some(iterator_proto.clone()), Class::Ordinary),
        array_buffer_proto: ordinary(),
        typed_protos: (0..10)
            .map(|_| Obj::new(Some(typed_base.clone()), Class::Ordinary))
            .chain(std::iter::once(typed_base.clone()))
            .collect(),
        iterator_proto,
        error_proto,
        function_proto,
        array_values: Value::Undefined,
        string_iterator: Value::Undefined,
        sym_iterator: symbol("Symbol.iterator"),
        sym_async_iterator: symbol("Symbol.asyncIterator"),
        sym_to_primitive: symbol("Symbol.toPrimitive"),
        sym_has_instance: symbol("Symbol.hasInstance"),
        sym_to_string_tag: symbol("Symbol.toStringTag"),
        sym_species: symbol("Symbol.species"),
        sym_concat_spreadable: symbol("Symbol.isConcatSpreadable"),
        sym_unscopables: symbol("Symbol.unscopables"),
        sym_match: symbol("Symbol.match"),
        sym_match_all: symbol("Symbol.matchAll"),
        sym_replace: symbol("Symbol.replace"),
        sym_search: symbol("Symbol.search"),
        sym_split: symbol("Symbol.split"),
        registry: Vec::new(),
        object_proto,
    };
    let global = Obj::new(Some(intrinsics.object_proto.clone()), Class::Ordinary);
    (global, intrinsics)
}

/// The built-in objects' methods and constructors, and the global object's names.
pub fn fill(interp: &mut crate::interp::Interp) {
    crate::builtins::fill(interp);
}
