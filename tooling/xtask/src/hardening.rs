// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask hardening-check <file>…`: a built program has the system's protections
//! (gaps row H13), read from its own headers, so a release job fails before one without them
//! is published:
//!
//! - Linux (ELF): position-independent (`ET_DYN`), relocations read-only once loaded (a
//!   `PT_GNU_RELRO` segment and `BIND_NOW`), a stack that cannot run code (`PT_GNU_STACK`
//!   without the execute flag), and no segment both writable and runnable.
//! - Windows (PE): address space randomised (`DYNAMIC_BASE`, `HIGH_ENTROPY_VA` on 64 bits),
//!   data not runnable (`NX_COMPAT`), Control Flow Guard (`GUARD_CF`), and no section both
//!   writable and runnable.
//! - macOS (Mach-O, each slice of a universal file): position-independent (`MH_PIE`), no
//!   segment both writable and runnable, and where it is signed by an identity (not ad hoc, as
//!   Rust's linker signs every build), the hardened runtime.
//!
//! dm's own readers of the three formats' headers, a few fields each; no third-party code.

use std::path::Path;

/// Reads little- or big-endian numbers at an offset, `None` past the end.
struct Bytes<'a> {
    data: &'a [u8],
    big: bool,
}

impl Bytes<'_> {
    fn take<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        self.data.get(at..at.checked_add(N)?)?.try_into().ok()
    }
    fn u16(&self, at: usize) -> Option<u16> {
        let b = self.take::<2>(at)?;
        Some(if self.big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }
    fn u32(&self, at: usize) -> Option<u32> {
        let b = self.take::<4>(at)?;
        Some(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
    fn u64(&self, at: usize) -> Option<u64> {
        let b = self.take::<8>(at)?;
        Some(if self.big {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        })
    }
}

/// What a file lacks, by its format; an empty list where it has every protection.
pub fn lacks(data: &[u8]) -> Result<Vec<String>, String> {
    if data.starts_with(b"\x7fELF") {
        elf(data)
    } else if data.starts_with(b"MZ") {
        pe(data)
    } else if macho_magic(data).is_some() || data.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
        macho_any(data)
    } else {
        Err("not a program this check reads (ELF, PE or Mach-O)".into())
    }
}

fn short() -> String {
    "the file ends inside its own headers".into()
}

/* ------------------------------------------------------------ ELF */

const PT_DYNAMIC: u32 = 2;
const PT_GNU_STACK: u32 = 0x6474_e551;
const PT_GNU_RELRO: u32 = 0x6474_e552;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const DT_FLAGS: u64 = 30;
const DT_FLAGS_1: u64 = 0x6fff_fffb;
const DF_BIND_NOW: u64 = 0x8;
const DF_1_NOW: u64 = 0x1;

fn elf(data: &[u8]) -> Result<Vec<String>, String> {
    let wide = data.get(4) == Some(&2);
    let b = Bytes {
        data,
        big: data.get(5) == Some(&2),
    };
    let ty = b.u16(16).ok_or_else(short)?;
    let (phoff, phentsize, phnum) = if wide {
        (
            b.u64(32).ok_or_else(short)? as usize,
            b.u16(54).ok_or_else(short)? as usize,
            b.u16(56).ok_or_else(short)? as usize,
        )
    } else {
        (
            b.u32(28).ok_or_else(short)? as usize,
            b.u16(42).ok_or_else(short)? as usize,
            b.u16(44).ok_or_else(short)? as usize,
        )
    };
    let mut lacks = Vec::new();
    let (mut relro, mut stack_seen, mut now) = (false, false, false);
    for i in 0..phnum {
        let at = phoff + i * phentsize;
        let kind = b.u32(at).ok_or_else(short)?;
        let flags = if wide { b.u32(at + 4) } else { b.u32(at + 24) }.ok_or_else(short)?;
        match kind {
            PT_GNU_RELRO => relro = true,
            PT_GNU_STACK => {
                stack_seen = true;
                if flags & PF_X != 0 {
                    lacks.push("its stack can run code (PT_GNU_STACK has X)".into());
                }
            }
            PT_LOAD if flags & PF_W != 0 && flags & PF_X != 0 => {
                lacks.push("a segment is both writable and runnable".into());
            }
            PT_DYNAMIC => {
                let (offset, size) = if wide {
                    (b.u64(at + 8), b.u64(at + 32))
                } else {
                    (b.u32(at + 4).map(u64::from), b.u32(at + 16).map(u64::from))
                };
                let (offset, size) = (offset.ok_or_else(short)?, size.ok_or_else(short)?);
                let entry = if wide { 16 } else { 8 };
                let mut at = offset as usize;
                while (at as u64) < offset + size {
                    let (tag, value) = if wide {
                        (b.u64(at), b.u64(at + 8))
                    } else {
                        (b.u32(at).map(u64::from), b.u32(at + 4).map(u64::from))
                    };
                    let (Some(tag), Some(value)) = (tag, value) else {
                        break;
                    };
                    if tag == 0 {
                        break;
                    }
                    if tag == DT_FLAGS && value & DF_BIND_NOW != 0
                        || tag == DT_FLAGS_1 && value & DF_1_NOW != 0
                    {
                        now = true;
                    }
                    at += entry;
                }
            }
            _ => {}
        }
    }
    // ET_DYN is a position-independent program or a library; ET_EXEC is a fixed address.
    if ty != 3 {
        lacks.push("it is not position-independent (ET_EXEC)".into());
    }
    if !relro {
        lacks.push("its relocations are not made read-only (no PT_GNU_RELRO)".into());
    }
    if !now {
        lacks.push("its symbols are not bound at load (no BIND_NOW), so RELRO is partial".into());
    }
    if !stack_seen {
        lacks.push("it does not say its stack cannot run code (no PT_GNU_STACK)".into());
    }
    Ok(lacks)
}

/* ------------------------------------------------------------ PE */

const HIGH_ENTROPY_VA: u16 = 0x0020;
const DYNAMIC_BASE: u16 = 0x0040;
const NX_COMPAT: u16 = 0x0100;
const GUARD_CF: u16 = 0x4000;
const SCN_EXECUTE: u32 = 0x2000_0000;
const SCN_WRITE: u32 = 0x8000_0000;

fn pe(data: &[u8]) -> Result<Vec<String>, String> {
    let b = Bytes { data, big: false };
    let header = b.u32(0x3c).ok_or_else(short)? as usize;
    if b.take::<4>(header) != Some(*b"PE\0\0") {
        return Err("an MZ file with no PE header".into());
    }
    let sections = b.u16(header + 6).ok_or_else(short)? as usize;
    let optional_size = b.u16(header + 20).ok_or_else(short)? as usize;
    let optional = header + 24;
    let wide = b.u16(optional).ok_or_else(short)? == 0x20b;
    let characteristics = b.u16(optional + 70).ok_or_else(short)?;
    let mut lacks = Vec::new();
    for (flag, what) in [
        (
            DYNAMIC_BASE,
            "its address is not randomised (no DYNAMIC_BASE)",
        ),
        (NX_COMPAT, "its data can run as code (no NX_COMPAT)"),
        (GUARD_CF, "it has no Control Flow Guard (no GUARD_CF)"),
    ] {
        if characteristics & flag == 0 {
            lacks.push(what.to_string());
        }
    }
    if wide && characteristics & HIGH_ENTROPY_VA == 0 {
        lacks.push("its address is randomised over 32 bits only (no HIGH_ENTROPY_VA)".into());
    }
    let table = optional + optional_size;
    for i in 0..sections {
        let at = table + i * 40;
        let flags = b.u32(at + 36).ok_or_else(short)?;
        if flags & SCN_EXECUTE != 0 && flags & SCN_WRITE != 0 {
            let name = data.get(at..at + 8).unwrap_or_default();
            let name = String::from_utf8_lossy(name)
                .trim_end_matches('\0')
                .to_string();
            lacks.push(format!("its section {name} is both writable and runnable"));
        }
    }
    Ok(lacks)
}

/* ------------------------------------------------------------ Mach-O */

const MH_PIE: u32 = 0x0020_0000;
const LC_SEGMENT: u32 = 0x1;
const LC_SEGMENT_64: u32 = 0x19;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const VM_PROT_WRITE: u32 = 0x2;
const VM_PROT_EXECUTE: u32 = 0x4;
const CS_ADHOC: u32 = 0x0000_0002;
const CS_LINKER_SIGNED: u32 = 0x0002_0000;
const CS_RUNTIME: u32 = 0x0001_0000;
const CSMAGIC_EMBEDDED_SIGNATURE: u32 = 0xfade_0cc0;
const CSMAGIC_CODEDIRECTORY: u32 = 0xfade_0c02;

/// A thin Mach-O's byte order and width, from its magic.
fn macho_magic(data: &[u8]) -> Option<(bool, bool)> {
    match data.get(..4)? {
        [0xfe, 0xed, 0xfa, 0xce] => Some((true, false)),
        [0xfe, 0xed, 0xfa, 0xcf] => Some((true, true)),
        [0xce, 0xfa, 0xed, 0xfe] => Some((false, false)),
        [0xcf, 0xfa, 0xed, 0xfe] => Some((false, true)),
        _ => None,
    }
}

/// A universal file's slices, each checked; a thin one checked as it is.
fn macho_any(data: &[u8]) -> Result<Vec<String>, String> {
    if macho_magic(data).is_some() {
        return macho(data);
    }
    let b = Bytes { data, big: true };
    let count = b.u32(4).ok_or_else(short)? as usize;
    let mut lacks = Vec::new();
    for i in 0..count {
        let at = 8 + i * 20;
        let offset = b.u32(at + 8).ok_or_else(short)? as usize;
        let size = b.u32(at + 12).ok_or_else(short)? as usize;
        let slice = data.get(offset..offset + size).ok_or_else(short)?;
        for lack in macho(slice)? {
            lacks.push(format!("slice {i}: {lack}"));
        }
    }
    Ok(lacks)
}

fn macho(data: &[u8]) -> Result<Vec<String>, String> {
    let (big, wide) = macho_magic(data).ok_or("not a Mach-O")?;
    let b = Bytes { data, big };
    let commands = b.u32(16).ok_or_else(short)? as usize;
    let flags = b.u32(24).ok_or_else(short)?;
    let mut lacks = Vec::new();
    let ty = b.u32(12).ok_or_else(short)?;
    // MH_EXECUTE alone carries MH_PIE; a library is position-independent by kind.
    if ty == 2 && flags & MH_PIE == 0 {
        lacks.push("it is not position-independent (no MH_PIE)".into());
    }
    let mut at = if wide { 32 } else { 28 };
    let mut signed = None;
    for _ in 0..commands {
        let cmd = b.u32(at).ok_or_else(short)?;
        let size = b.u32(at + 4).ok_or_else(short)? as usize;
        match cmd {
            LC_SEGMENT | LC_SEGMENT_64 => {
                let prot_at = if cmd == LC_SEGMENT_64 {
                    at + 60
                } else {
                    at + 44
                };
                let initial = b.u32(prot_at).ok_or_else(short)?;
                if initial & VM_PROT_WRITE != 0 && initial & VM_PROT_EXECUTE != 0 {
                    let name = data.get(at + 8..at + 24).unwrap_or_default();
                    let name = String::from_utf8_lossy(name)
                        .trim_end_matches('\0')
                        .to_string();
                    lacks.push(format!("its segment {name} is both writable and runnable"));
                }
            }
            LC_CODE_SIGNATURE => {
                let offset = b.u32(at + 8).ok_or_else(short)? as usize;
                signed = Some(hardened(data, offset));
            }
            _ => {}
        }
        if size == 0 {
            return Err("a load command of no length".into());
        }
        at += size;
    }
    if signed == Some(Signed::WithoutRuntime) {
        lacks.push("it is signed without the hardened runtime".into());
    }
    Ok(lacks)
}

/// How a Mach-O is signed, as far as the hardened runtime goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signed {
    /// Ad hoc or by the linker: no identity, so no hardened runtime to ask of it.
    AdHoc,
    /// By an identity, with the hardened runtime.
    WithRuntime,
    /// By an identity, without it.
    WithoutRuntime,
}

/// How the code signature at `offset` is made: its code directory's flags. A signature is
/// big-endian whatever the program's order.
fn hardened(data: &[u8], offset: usize) -> Signed {
    let b = Bytes { data, big: true };
    if b.u32(offset) != Some(CSMAGIC_EMBEDDED_SIGNATURE) {
        return Signed::WithoutRuntime;
    }
    let count = b.u32(offset + 8).unwrap_or(0) as usize;
    let flags = (0..count).find_map(|i| {
        let blob = b.u32(offset + 12 + i * 8 + 4).unwrap_or(0) as usize;
        let at = offset + blob;
        (b.u32(at) == Some(CSMAGIC_CODEDIRECTORY))
            .then(|| b.u32(at + 12))
            .flatten()
    });
    match flags {
        Some(f) if f & (CS_ADHOC | CS_LINKER_SIGNED) != 0 => Signed::AdHoc,
        Some(f) if f & CS_RUNTIME != 0 => Signed::WithRuntime,
        _ => Signed::WithoutRuntime,
    }
}

/// Checks each of `files`, saying what each lacks; an error where any lacks something.
pub fn run(files: &[String]) -> Result<String, String> {
    if files.is_empty() {
        return Err("usage: cargo xtask hardening-check <program>…".into());
    }
    let mut failed = Vec::new();
    for file in files {
        let data = std::fs::read(Path::new(file)).map_err(|e| format!("{file}: {e}"))?;
        match lacks(&data) {
            Ok(list) if list.is_empty() => {}
            Ok(list) => failed.push(format!("{file}:\n  - {}", list.join("\n  - "))),
            Err(why) => failed.push(format!("{file}: {why}")),
        }
    }
    if failed.is_empty() {
        Ok(format!(
            "{} program(s) have the system's protections",
            files.len()
        ))
    } else {
        Err(failed.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The program running this test was built by the same compiler for this system: on
    /// macOS and Linux, position-independent and without a writable runnable segment.
    #[test]
    fn the_running_test_reads_as_a_program_of_this_system() {
        let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        let found = lacks(&me).unwrap();
        assert!(
            !found.iter().any(|l| l.contains("writable and runnable")),
            "{found:?}"
        );
        // Built here and signed ad hoc by the linker: nothing lacking at all.
        #[cfg(target_os = "macos")]
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_pe_without_its_flags_lacks_each() {
        // MZ, e_lfanew = 0x40, "PE\0\0", one section, a PE32+ optional header of 240 bytes.
        let mut pe = vec![0u8; 0x40 + 24 + 240 + 40];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x40;
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        pe[0x40 + 6] = 1;
        pe[0x40 + 20] = 240;
        let optional = 0x40 + 24;
        pe[optional..optional + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        let section = optional + 240;
        pe[section..section + 5].copy_from_slice(b".text");
        pe[section + 36..section + 40].copy_from_slice(&(SCN_EXECUTE | SCN_WRITE).to_le_bytes());
        let found = lacks(&pe).unwrap();
        assert_eq!(found.len(), 5, "{found:?}");
        let all = DYNAMIC_BASE | NX_COMPAT | GUARD_CF | HIGH_ENTROPY_VA;
        pe[optional + 70..optional + 72].copy_from_slice(&all.to_le_bytes());
        pe[section + 36..section + 40].copy_from_slice(&SCN_EXECUTE.to_le_bytes());
        assert!(lacks(&pe).unwrap().is_empty());
    }

    #[test]
    fn a_file_that_is_no_program_is_said() {
        assert!(lacks(b"%PDF-1.7").is_err());
        assert!(lacks(b"\x7fELF").is_err());
    }
}
