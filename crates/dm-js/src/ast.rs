// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A program as the parser reads it: statements, expressions and the patterns a value is taken
//! apart by. Shared pieces (a function's body, a name) are `Rc`, so the interpreter holds them
//! without copying.

use std::rc::Rc;

use crate::lexer::Text;

/// A name, as written.
pub type Name = Rc<str>;

/// `var`, `let` or `const`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    /// Function-wide.
    Var,
    /// Block-wide, changeable.
    Let,
    /// Block-wide, set once.
    Const,
}

/// A statement.
#[derive(Debug)]
pub enum Stmt {
    /// `var a = 1, b;`
    Var(VarKind, Vec<(Pattern, Option<Expr>)>),
    /// `function f() {}`
    Function(Rc<Function>),
    /// `class C {}`
    Class(Rc<Class>),
    /// An expression on its own.
    Expr(Expr),
    /// `{ … }`
    Block(Vec<Stmt>),
    /// `;`
    Empty,
    /// `if (test) then else otherwise`
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    /// `for (init; test; update) body`
    For {
        /// Before the first turn.
        init: Option<Box<Stmt>>,
        /// Before each turn.
        test: Option<Expr>,
        /// After each turn.
        update: Option<Expr>,
        /// Each turn.
        body: Box<Stmt>,
    },
    /// `for (left in right) body`, or `of` where `of` is true.
    ForIn {
        /// What each key or value is put in.
        left: ForTarget,
        /// The object or the iterable.
        right: Expr,
        /// Each turn.
        body: Box<Stmt>,
        /// `for … of`.
        of: bool,
    },
    /// `while (test) body`
    While(Expr, Box<Stmt>),
    /// `do body while (test)`
    DoWhile(Box<Stmt>, Expr),
    /// `return value;`
    Return(Option<Expr>),
    /// `break label;`
    Break(Option<Name>),
    /// `continue label;`
    Continue(Option<Name>),
    /// `throw value;`
    Throw(Expr),
    /// `try { } catch (param) { } finally { }`
    Try {
        /// The block tried.
        block: Vec<Stmt>,
        /// What a thrown value is caught as (none in `catch {}`), and the catching block.
        handler: Option<(Option<Pattern>, Vec<Stmt>)>,
        /// What runs after either.
        finalizer: Option<Vec<Stmt>>,
    },
    /// `switch (value) { case …: … default: … }`
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    /// `label: body`
    Labeled(Name, Box<Stmt>),
    /// `debugger;`, which does nothing here.
    Debugger,
}

/// What a `for … in` or `for … of` sets on each turn.
#[derive(Debug)]
pub enum ForTarget {
    /// `for (var x in …)`, `let`, `const`.
    Declare(VarKind, Pattern),
    /// `for (x in …)`, `for (a.b in …)`.
    Assign(Pattern),
}

/// A pattern a value is taken apart by (`[a, b] = …`, `{x, y: z} = …`), or a plain name or
/// place.
#[derive(Debug)]
pub enum Pattern {
    /// A name.
    Name(Name),
    /// A place an assignment reaches (`a.b`, `a[b]`), in an assignment's left side only.
    Place(Box<Expr>),
    /// `[a, , b, ...rest]`
    Array(Vec<Option<Pattern>>, Option<Box<Pattern>>),
    /// `{a, b: c, [k]: d, ...rest}`
    Object(Vec<(PropKey, Pattern)>, Option<Box<Pattern>>),
    /// `target = default`
    Default(Box<Pattern>, Box<Expr>),
}

/// An object literal's or a pattern's key.
#[derive(Debug)]
pub enum PropKey {
    /// `a:`, `"a":`, `1:`
    Fixed(Text),
    /// `[expr]:`
    Computed(Box<Expr>),
    /// `#a` in a class.
    Private(Name),
}

/// What an object literal holds.
#[derive(Debug)]
pub enum Prop {
    /// `key: value`, `key` (`a` for `a: a`), `key() {}`
    Value(PropKey, Expr),
    /// `get key() {}`
    Getter(PropKey, Rc<Function>),
    /// `set key(v) {}`
    Setter(PropKey, Rc<Function>),
    /// `...other`
    Spread(Expr),
}

/// An argument or array element, spread or not.
#[derive(Debug)]
pub struct Arg {
    /// `...value`
    pub spread: bool,
    /// The value.
    pub value: Expr,
}

/// A unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unary {
    /// `-`
    Neg,
    /// `+`
    Plus,
    /// `!`
    Not,
    /// `~`
    BitNot,
    /// `typeof`
    Typeof,
    /// `void`
    Void,
    /// `delete`
    Delete,
}

/// A binary operator (not `&&`, `||` or `??`, which are [`Logical`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binary {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `**`
    Pow,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `===`
    StrictEq,
    /// `!==`
    StrictNe,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `>>>`
    UShr,
    /// `in`
    In,
    /// `instanceof`
    Instanceof,
}

/// `&&`, `||`, `??`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logical {
    /// `&&`
    And,
    /// `||`
    Or,
    /// `??`
    Nullish,
}

/// An expression.
#[derive(Debug)]
pub enum Expr {
    /// A number.
    Number(f64),
    /// A string.
    String(Text),
    /// A BigInt, its base-ten digits.
    BigInt(Name),
    /// A pattern literal: source and flags.
    Pattern(Name, Name),
    /// A template: its pieces (each `None` where a tagged template's escape was not valid) and
    /// raw text, and the values between them.
    Template(Rc<Template>, Vec<Expr>),
    /// `` tag`…` ``
    Tagged(Box<Expr>, Rc<Template>, Vec<Expr>),
    /// A name.
    Name(Name),
    /// `this`
    This,
    /// `super` (in a method, before `.x`, `[x]` or `(…)`).
    Super,
    /// `new.target`
    NewTarget,
    /// `true`, `false`.
    Bool(bool),
    /// `null`
    Null,
    /// `[a, , ...b]`
    Array(Vec<Option<Arg>>),
    /// `{a: 1, b, ...c}`
    Object(Vec<Prop>),
    /// `function () {}`, `() => {}`, a method.
    Function(Rc<Function>),
    /// `class {}`
    Class(Rc<Class>),
    /// `-a`, `typeof a`, …
    Unary(Unary, Box<Expr>),
    /// `++a`, `a--` (prefix when the first is true; `true` for `++`).
    Update {
        /// `++a` rather than `a++`.
        prefix: bool,
        /// `++` rather than `--`.
        up: bool,
        /// What is changed.
        target: Box<Expr>,
    },
    /// `a + b`, …
    Binary(Binary, Box<Expr>, Box<Expr>),
    /// `a && b`, `a || b`, `a ?? b`
    Logical(Logical, Box<Expr>, Box<Expr>),
    /// `a = b`, `a += b` (the operator `None` for `=`, a [`Logical`] in `Assign::Logical`).
    Assign(Assign, Box<Pattern>, Box<Expr>),
    /// `test ? then : otherwise`
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `f(a, b)`, `f?.(a)`
    Call {
        /// What is called.
        callee: Box<Expr>,
        /// With what.
        args: Vec<Arg>,
        /// `?.(`: nothing is called when the callee is null or undefined.
        optional: bool,
    },
    /// `new C(a)`
    New(Box<Expr>, Vec<Arg>),
    /// `a.b`, `a[b]`, `a?.b`, `a.#b`
    Member {
        /// The object.
        object: Box<Expr>,
        /// The key.
        key: Box<MemberKey>,
        /// `?.`
        optional: bool,
    },
    /// An optional chain's end: where a `?.` in it met null or undefined, the whole is
    /// undefined.
    Chain(Box<Expr>),
    /// `a, b`
    Sequence(Vec<Expr>),
    /// `yield value`, `yield* value` (the second `true`).
    Yield(Option<Box<Expr>>, bool),
    /// `await value`
    Await(Box<Expr>),
    /// A parenthesised list only an arrow function's parameters take (`()`, `(a, ...b)`),
    /// read before its `=>`: never left in a program once read.
    ArrowParams(Vec<Arg>),
}

/// A member's key.
#[derive(Debug)]
pub enum MemberKey {
    /// `.name`
    Name(Name),
    /// `[expr]`
    Computed(Expr),
    /// `.#name`
    Private(Name),
}

/// An assignment's operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assign {
    /// `=`
    Set,
    /// `+=`, `*=`, …
    With(Binary),
    /// `&&=`, `||=`, `??=`
    Logical(Logical),
}

/// A template literal's text.
#[derive(Debug)]
pub struct Template {
    /// Each piece's value.
    pub cooked: Vec<Option<Text>>,
    /// Each piece as written.
    pub raw: Vec<Text>,
}

/// A function: declared, written as a value, an arrow, a method, a getter or a setter.
#[derive(Debug)]
pub struct Function {
    /// Its name, where it has one.
    pub name: Option<Name>,
    /// Its parameters (the last may be `...rest`).
    pub params: Vec<Pattern>,
    /// `...rest`, after the others.
    pub rest: Option<Pattern>,
    /// Its body; an arrow's expression is a `return`.
    pub body: Vec<Stmt>,
    /// An arrow: `this` and `arguments` are the surrounding function's.
    pub arrow: bool,
    /// `function*`
    pub generator: bool,
    /// `async`
    pub is_async: bool,
    /// The function's body starts with `"use strict"` (or it is in a class).
    pub strict: bool,
    /// Where it starts and ends in the text (for `toString`).
    pub span: (usize, usize),
    /// A method (of a class or an object literal): it has no `prototype` and can reach `super`.
    pub method: bool,
}

/// A class.
#[derive(Debug)]
pub struct Class {
    /// Its name, where it has one.
    pub name: Option<Name>,
    /// `extends value`
    pub extends: Option<Expr>,
    /// Its constructor, where it writes one.
    pub constructor: Option<Rc<Function>>,
    /// Its methods, getters, setters and fields.
    pub members: Vec<ClassMember>,
}

/// One of a class's members.
#[derive(Debug)]
pub struct ClassMember {
    /// `static`
    pub is_static: bool,
    /// Its key.
    pub key: PropKey,
    /// What it is.
    pub kind: MemberKind,
}

/// What a class member is.
#[derive(Debug)]
pub enum MemberKind {
    /// A method.
    Method(Rc<Function>),
    /// `get x() {}`
    Getter(Rc<Function>),
    /// `set x(v) {}`
    Setter(Rc<Function>),
    /// `x = value;` (set on each object made, or on the class where static).
    Field(Option<Expr>),
    /// `static { … }`
    StaticBlock(Rc<Function>),
}
