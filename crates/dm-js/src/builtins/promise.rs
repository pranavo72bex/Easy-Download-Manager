// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Promise`. What waits on a promise runs as a job once the code running now is done
//! ([`Interp::run_jobs`]); `await` runs the jobs until its promise settles, since nothing a
//! program here waits on comes from outside it.

use super::{arg, constructor, method, own_data, with_data};
use crate::interp::{Interp, Res};
use crate::object::{Class, Obj};
use crate::promise::PromiseState;
use crate::value::{Key, Value};

/// Installs `Promise`.
pub fn install(interp: &mut Interp) {
    let pp = interp.intrinsics.promise_proto.clone();
    let promise = constructor(
        interp,
        "Promise",
        1,
        |i, _, a, made| {
            let Some(proto) = made else {
                return Err(i.type_error("Promise constructor cannot be invoked without 'new'"));
            };
            let executor = arg(a, 0);
            if !matches!(&executor, Value::Object(f) if f.is_callable()) {
                return Err(i.type_error("Promise resolver is not a function"));
            }
            let p = Obj::new(
                Some(proto.clone()),
                Class::Promise(PromiseState::Pending(Vec::new())),
            );
            let (ok, err) = resolvers(i, &p);
            if let Err(e) = i.call(&executor, Value::Undefined, &[ok, err]) {
                reject(i, &p, e);
            }
            Ok(Value::Object(p))
        },
        &pp,
    );
    let tag = Key::Symbol(interp.intrinsics.sym_to_string_tag.clone());
    pp.set_hidden(tag, Value::str("Promise"));
    method(interp, &pp, "then", 2, |i, this, a, _| {
        let p = this_promise(i, this)?;
        Ok(Value::Object(then(i, &p, arg(a, 0), arg(a, 1))))
    });
    method(interp, &pp, "catch", 1, |i, this, a, _| {
        let p = this_promise(i, this)?;
        Ok(Value::Object(then(i, &p, Value::Undefined, arg(a, 0))))
    });
    method(interp, &pp, "finally", 1, |i, this, a, _| {
        let p = this_promise(i, this)?;
        let f = arg(a, 0);
        let ok = with_data(
            i,
            "",
            1,
            |i, _, a, _| {
                let f = own_data(i);
                if f.as_object().is_some_and(|o| o.is_callable()) {
                    i.call(&f, Value::Undefined, &[])?;
                }
                Ok(arg(a, 0))
            },
            f.clone(),
        );
        let err = with_data(
            i,
            "",
            1,
            |i, _, a, _| {
                let f = own_data(i);
                if f.as_object().is_some_and(|o| o.is_callable()) {
                    i.call(&f, Value::Undefined, &[])?;
                }
                Err(arg(a, 0))
            },
            f,
        );
        Ok(Value::Object(then(
            i,
            &p,
            Value::Object(ok),
            Value::Object(err),
        )))
    });
    method(interp, &promise, "resolve", 1, |i, _, a, _| {
        let v = arg(a, 0);
        if let Value::Object(o) = &v {
            if matches!(o.borrow().class, Class::Promise(_)) {
                return Ok(v);
            }
        }
        Ok(resolved(i, v))
    });
    method(interp, &promise, "reject", 1, |i, _, a, _| {
        Ok(rejected(i, arg(a, 0)))
    });
    method(interp, &promise, "all", 1, |i, _, a, _| {
        combine(i, arg(a, 0), 0)
    });
    method(interp, &promise, "allSettled", 1, |i, _, a, _| {
        combine(i, arg(a, 0), 1)
    });
    method(interp, &promise, "any", 1, |i, _, a, _| {
        combine(i, arg(a, 0), 2)
    });
    method(interp, &promise, "race", 1, |i, _, a, _| {
        combine(i, arg(a, 0), 3)
    });
}

fn this_promise(i: &mut Interp, this: &Value) -> Res<Obj> {
    match this {
        Value::Object(o) if matches!(o.borrow().class, Class::Promise(_)) => Ok(o.clone()),
        _ => Err(i.type_error("not a promise")),
    }
}

fn new_promise(i: &Interp) -> Obj {
    Obj::new(
        Some(i.intrinsics.promise_proto.clone()),
        Class::Promise(PromiseState::Pending(Vec::new())),
    )
}

/// A promise fulfilled with `v`.
pub fn resolved(i: &mut Interp, v: Value) -> Value {
    let p = new_promise(i);
    resolve(i, &p, v);
    Value::Object(p)
}

/// A promise rejected with `e`.
pub fn rejected(i: &mut Interp, e: Value) -> Value {
    let p = new_promise(i);
    reject(i, &p, e);
    Value::Object(p)
}

/// The resolve and reject functions of `p`.
fn resolvers(i: &Interp, p: &Obj) -> (Value, Value) {
    let ok = with_data(
        i,
        "",
        1,
        |i, _, a, _| {
            if let Value::Object(p) = own_data(i) {
                resolve(i, &p, arg(a, 0));
            }
            Ok(Value::Undefined)
        },
        Value::Object(p.clone()),
    );
    let err = with_data(
        i,
        "",
        1,
        |i, _, a, _| {
            if let Value::Object(p) = own_data(i) {
                reject(i, &p, arg(a, 0));
            }
            Ok(Value::Undefined)
        },
        Value::Object(p.clone()),
    );
    (Value::Object(ok), Value::Object(err))
}

fn pending(p: &Obj) -> bool {
    matches!(p.borrow().class, Class::Promise(PromiseState::Pending(_)))
}

/// Resolves `p` with `v`: a promise or thenable is followed, anything else fulfils it.
pub fn resolve(i: &mut Interp, p: &Obj, v: Value) {
    if !pending(p) {
        return;
    }
    if let Value::Object(o) = &v {
        if o.same(p) {
            let e = i.type_error("Chaining cycle detected for promise");
            return settle(i, p, Err(e));
        }
        // Some(Some(outcome)): a settled promise; Some(None): a pending one; None: not ours.
        let state = match &o.borrow().class {
            Class::Promise(PromiseState::Fulfilled(x)) => Some(Some(Ok(x.clone()))),
            Class::Promise(PromiseState::Rejected(x)) => Some(Some(Err(x.clone()))),
            Class::Promise(PromiseState::Pending(_)) => Some(None),
            _ => None,
        };
        let o = o.clone();
        let Some(state) = state else {
            // A thenable other than a promise of ours.
            let then = match i.get(&v, &Key::str("then")) {
                Ok(t) => t,
                Err(e) => return settle(i, p, Err(e)),
            };
            if then.as_object().is_some_and(|f| f.is_callable()) {
                let (ok, err) = resolvers(i, p);
                if let Err(e) = i.call(&then, v.clone(), &[ok, err]) {
                    settle(i, p, Err(e));
                }
                return;
            }
            return settle(i, p, Ok(v));
        };
        match state {
            Some(outcome) => return settle(i, p, outcome),
            None => {
                // Follows `o`: settled as it is settled.
                if let Class::Promise(PromiseState::Pending(waiting)) = &mut o.borrow_mut().class {
                    waiting.push((None, None, p.clone()));
                }
                return;
            }
        }
    }
    settle(i, p, Ok(v));
}

/// Rejects `p` with `e`.
pub fn reject(i: &mut Interp, p: &Obj, e: Value) {
    if pending(p) {
        settle(i, p, Err(e));
    }
}

/// Settles `p`, and queues what waited on it.
fn settle(i: &mut Interp, p: &Obj, outcome: Result<Value, Value>) {
    let waiting = {
        let mut me = p.borrow_mut();
        let Class::Promise(state) = &mut me.class else {
            return;
        };
        let new = match &outcome {
            Ok(v) => PromiseState::Fulfilled(v.clone()),
            Err(e) => PromiseState::Rejected(e.clone()),
        };
        match std::mem::replace(state, new) {
            PromiseState::Pending(w) => w,
            other => {
                *state = other;
                return;
            }
        }
    };
    for (on_ok, on_err, next) in waiting {
        queue(i, on_ok, on_err, next, outcome.clone());
    }
}

/// A job: `next` settled by what the handler for `outcome` gives.
fn queue(
    i: &mut Interp,
    on_ok: Option<Obj>,
    on_err: Option<Obj>,
    next: Obj,
    outcome: Result<Value, Value>,
) {
    let (handler, value, ok) = match outcome {
        Ok(v) => (on_ok, v, true),
        Err(e) => (on_err, e, false),
    };
    let data = i.array(vec![
        handler.map(Value::Object).unwrap_or(Value::Undefined),
        Value::Object(next),
        Value::Bool(ok),
    ]);
    let job = with_data(
        i,
        "",
        1,
        |i, _, a, _| {
            let data = own_data(i);
            let handler = i.get(&data, &Key::index(0))?;
            let Value::Object(next) = i.get(&data, &Key::index(1))? else {
                return Ok(Value::Undefined);
            };
            let ok = i.get(&data, &Key::index(2))?.truthy();
            let value = arg(a, 0);
            if handler.as_object().is_some_and(|f| f.is_callable()) {
                match i.call(&handler, Value::Undefined, &[value]) {
                    Ok(r) => resolve(i, &next, r),
                    Err(e) => reject(i, &next, e),
                }
            } else if ok {
                resolve(i, &next, value);
            } else {
                reject(i, &next, value);
            }
            Ok(Value::Undefined)
        },
        data,
    );
    i.jobs.push_back((job, value));
}

/// `p.then(on_ok, on_err)`: the promise of what they give.
pub fn then(i: &mut Interp, p: &Obj, on_ok: Value, on_err: Value) -> Obj {
    let next = new_promise(i);
    let callable = |v: Value| v.as_object().filter(|f| f.is_callable()).cloned();
    let (on_ok, on_err) = (callable(on_ok), callable(on_err));
    let settled = match &mut p.borrow_mut().class {
        Class::Promise(PromiseState::Pending(waiting)) => {
            waiting.push((on_ok.clone(), on_err.clone(), next.clone()));
            None
        }
        Class::Promise(PromiseState::Fulfilled(v)) => Some(Ok(v.clone())),
        Class::Promise(PromiseState::Rejected(e)) => Some(Err(e.clone())),
        _ => None,
    };
    if let Some(outcome) = settled {
        queue(i, on_ok, on_err, next.clone(), outcome);
    }
    next
}

/// What `await v` gives: `v`'s value once settled (its reason thrown), the jobs run until it
/// is; any other value as it is.
pub fn settled_value(i: &mut Interp, v: Value) -> Res<Value> {
    let Value::Object(o) = &v else {
        return Ok(v);
    };
    if !matches!(o.borrow().class, Class::Promise(_)) {
        let then = i.get(&v, &Key::str("then"))?;
        if !then.as_object().is_some_and(|f| f.is_callable()) {
            return Ok(v);
        }
        let p = resolved(i, v);
        return settled_value(i, p);
    }
    loop {
        match &o.borrow().class {
            Class::Promise(PromiseState::Fulfilled(x)) => return Ok(x.clone()),
            Class::Promise(PromiseState::Rejected(e)) => return Err(e.clone()),
            _ => {}
        }
        let Some((job, arg)) = i.jobs.pop_front() else {
            return Err(i.type_error("awaited a promise that never settles"));
        };
        i.call(&Value::Object(job), Value::Undefined, &[arg])?;
    }
}

/// `Promise.all` (0), `allSettled` (1), `any` (2), `race` (3).
fn combine(i: &mut Interp, items: Value, how: u8) -> Res<Value> {
    let list: Vec<Value> = i.iterate(items)?.collect::<Res<_>>()?;
    let out = new_promise(i);
    if list.is_empty() {
        match how {
            2 => {
                let e = i.error_of(
                    &i.intrinsics.error_proto.clone(),
                    "All promises were rejected",
                );
                reject(i, &out, e);
            }
            3 => {}
            _ => {
                let empty = i.array(Vec::new());
                resolve(i, &out, empty);
            }
        }
        return Ok(Value::Object(out));
    }
    // What has come back, by place, and how many are still to come.
    let state = i.array(vec![Value::Undefined; list.len()]);
    let left = i.plain_object();
    left.set_own(Key::str("n"), Value::Number(list.len() as f64));
    for (n, item) in list.into_iter().enumerate() {
        let p = match resolved(i, item) {
            Value::Object(p) => p,
            _ => continue,
        };
        let data = i.array(vec![
            Value::Object(out.clone()),
            state.clone(),
            Value::Object(left.clone()),
            Value::Number(n as f64),
            Value::Number(how as f64),
        ]);
        let ok = with_data(
            i,
            "",
            1,
            |i, _, a, _| collected(i, arg(a, 0), true),
            data.clone(),
        );
        let err = with_data(i, "", 1, |i, _, a, _| collected(i, arg(a, 0), false), data);
        then(i, &p, Value::Object(ok), Value::Object(err));
    }
    Ok(Value::Object(out))
}

fn collected(i: &mut Interp, v: Value, ok: bool) -> Res<Value> {
    let data = own_data(i);
    let Value::Object(out) = i.get(&data, &Key::index(0))? else {
        return Ok(Value::Undefined);
    };
    let state = i.get(&data, &Key::index(1))?;
    let left = i.get(&data, &Key::index(2))?;
    let n = i.get(&data, &Key::index(3))?;
    let how = i.get(&data, &Key::index(4))?;
    let n = i.to_number(&n)? as usize;
    let how = i.to_number(&how)? as u8;
    let finish = |i: &mut Interp, record: Value| -> Res<bool> {
        i.set(&state, Key::index(n), record)?;
        let k = i.get(&left, &Key::str("n"))?;
        let k = i.to_number(&k)? - 1.0;
        i.set(&left, Key::str("n"), Value::Number(k))?;
        Ok(k <= 0.0)
    };
    match (how, ok) {
        (3, true) => resolve(i, &out, v),
        (3, false) | (0, false) => reject(i, &out, v),
        (2, true) => resolve(i, &out, v),
        (0, true) => {
            if finish(i, v)? {
                resolve(i, &out, state);
            }
        }
        (2, false) => {
            if finish(i, v)? {
                let e = i.error_of(
                    &i.intrinsics.error_proto.clone(),
                    "All promises were rejected",
                );
                if let Value::Object(eo) = &e {
                    eo.set_hidden(Key::str("errors"), state.clone());
                }
                reject(i, &out, e);
            }
        }
        _ => {
            let r = i.plain_object();
            r.set_own(
                Key::str("status"),
                Value::str(if ok { "fulfilled" } else { "rejected" }),
            );
            r.set_own(Key::str(if ok { "value" } else { "reason" }), v);
            if finish(i, Value::Object(r))? {
                resolve(i, &out, state);
            }
        }
    }
    Ok(Value::Undefined)
}
