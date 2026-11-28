// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The contexts YouTube's framed stream hands its player to send back (gaps row Y2). A context
//! update ([`kind::SABR_CONTEXT_UPDATE`]) gives one by its type: a value kept as it came,
//! over the one held or not as its write policy says, and sent from then on where it is to be
//! sent by default. A sending policy ([`kind::SABR_CONTEXT_SENDING_POLICY`]) starts, stops or
//! forgets types by number. Each request carries those to be sent (`StreamerContext`
//! [`streamer::SENT`], each {type, value}) and names the types held but not sent
//! ([`streamer::UNSENT`]).
//!
//! The numbers are those of the published descriptions of the protocol, as they stood in
//! 2026; none of them was checked against YouTube's own servers here.

use std::collections::{BTreeMap, BTreeSet};

use super::messages::kind;
use super::wire::{self, Message, Value};

/// `SabrContextUpdate`'s fields.
pub mod update {
    /// The context's type, a number.
    pub const TYPE: u32 = 1;
    /// Its scope (1 the playback, 2 the request, 3 the watch page, 4 the ads): kept, not used.
    pub const SCOPE: u32 = 2;
    /// Its value, sent back as it came.
    pub const VALUE: u32 = 3;
    /// Whether it is sent without a policy's start.
    pub const SEND_BY_DEFAULT: u32 = 4;
    /// Its write policy: [`OVERWRITE`] or [`KEEP_EXISTING`].
    pub const WRITE_POLICY: u32 = 5;
    /// The write policy: the value held replaced.
    pub const OVERWRITE: u64 = 1;
    /// The write policy: the value held kept, where one is.
    pub const KEEP_EXISTING: u64 = 2;
}

/// `SabrContextSendingPolicy`'s fields: repeated context types.
pub mod policy {
    /// The types to send from now on.
    pub const START: u32 = 1;
    /// The types to send no more (still held).
    pub const STOP: u32 = 2;
    /// The types forgotten.
    pub const DISCARD: u32 = 3;
}

/// The fields of the request's `StreamerContext` the contexts go in.
pub mod streamer {
    /// Each context sent (`SabrContext`: [`TYPE`] and [`VALUE`]).
    pub const SENT: u32 = 5;
    /// Each type held but not sent.
    pub const UNSENT: u32 = 6;
    /// A sent context's type.
    pub const TYPE: u32 = 1;
    /// A sent context's value.
    pub const VALUE: u32 = 2;
}

/// One context update, as the part says it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Update {
    /// The context's type.
    pub kind: u64,
    /// Its scope, where the part says one.
    pub scope: Option<u64>,
    /// Its value.
    pub value: Vec<u8>,
    /// Sent without a policy's start.
    pub send: bool,
    /// The value held kept, where one is.
    pub keep_existing: bool,
}

impl Update {
    /// The update in `data`, where it names a type and a value.
    pub fn read(data: &[u8]) -> Option<Update> {
        let m = Message(data);
        Some(Update {
            kind: m.uint(update::TYPE)?,
            scope: m.uint(update::SCOPE),
            value: m.bytes(update::VALUE)?.to_vec(),
            send: m.flag(update::SEND_BY_DEFAULT).unwrap_or(false),
            keep_existing: m.uint(update::WRITE_POLICY) == Some(update::KEEP_EXISTING),
        })
    }
}

/// A sending policy: the types started, stopped and forgotten.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendingPolicy {
    /// Sent from now on.
    pub start: Vec<u64>,
    /// Sent no more.
    pub stop: Vec<u64>,
    /// Forgotten.
    pub discard: Vec<u64>,
}

impl SendingPolicy {
    /// The policy in `data`.
    pub fn read(data: &[u8]) -> SendingPolicy {
        let m = Message(data);
        SendingPolicy {
            start: numbers(m, policy::START),
            stop: numbers(m, policy::STOP),
            discard: numbers(m, policy::DISCARD),
        }
    }
}

/// Every number of repeated field `number`, written one by one or packed into bytes.
fn numbers(m: Message<'_>, number: u32) -> Vec<u64> {
    let mut all = Vec::new();
    for (_, value) in m.fields().filter(|(n, _)| *n == number) {
        match value {
            Value::Varint(v) => all.push(v),
            Value::Bytes(mut packed) => {
                while let Some((v, used)) = wire::varint(packed) {
                    all.push(v);
                    packed = &packed[used..];
                }
            }
            Value::Fixed32(_) | Value::Fixed64(_) => {}
        }
    }
    all
}

/// The contexts held over one fetch, by type, and those to be sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contexts {
    values: BTreeMap<u64, Vec<u8>>,
    sending: BTreeSet<u64>,
}

impl Contexts {
    /// What part `part` says, taken where it is an update or a sending policy; whether it was.
    pub fn take(&mut self, part: u64, data: &[u8]) -> bool {
        match part {
            kind::SABR_CONTEXT_UPDATE => {
                if let Some(update) = Update::read(data) {
                    self.update(update);
                }
                true
            }
            kind::SABR_CONTEXT_SENDING_POLICY => {
                self.follow(&SendingPolicy::read(data));
                true
            }
            _ => false,
        }
    }

    /// An update kept: over the value held unless it says to keep that one.
    pub fn update(&mut self, update: Update) {
        if update.keep_existing && self.values.contains_key(&update.kind) {
            return;
        }
        if update.send {
            self.sending.insert(update.kind);
        }
        self.values.insert(update.kind, update.value);
    }

    /// A sending policy followed.
    pub fn follow(&mut self, policy: &SendingPolicy) {
        self.sending.extend(policy.start.iter().copied());
        for ty in &policy.stop {
            self.sending.remove(ty);
        }
        for ty in &policy.discard {
            self.values.remove(ty);
        }
    }

    /// The contexts held that are to be sent, by type.
    pub fn sent(&self) -> Vec<(u64, &[u8])> {
        self.values
            .iter()
            .filter(|(ty, _)| self.sending.contains(*ty))
            .map(|(ty, value)| (*ty, value.as_slice()))
            .collect()
    }

    /// The types held that are not to be sent.
    pub fn unsent(&self) -> Vec<u64> {
        self.values
            .keys()
            .filter(|ty| !self.sending.contains(*ty))
            .copied()
            .collect()
    }
}
