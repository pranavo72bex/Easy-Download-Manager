// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux: a seccomp filter on every thread of the process (`SECCOMP_FILTER_FLAG_TSYNC`) and on
//! every thread made later, written as the kernel's BPF here, no library. A call it names is
//! refused with `EPERM` (a program sees a failure, not a crash); everything else the engine
//! needs (memory, threads, futexes, reading, writing to what is open) goes on.
//!
//! Refused: starting a program; making a process (`clone` without `CLONE_THREAD`; `clone3`,
//! whose flags a filter cannot read, says `ENOSYS`, and the C library falls back to `clone`);
//! any socket; opening a file to write, make, add to or cut; making, moving, linking or
//! removing a file or folder, or changing its owner or rights; another process read, written
//! or signalled; mounting, namespaces, kernel keys, BPF and performance counters. A call made
//! in another architecture's numbering (x32 on x86-64) ends the process.

/// The architectures the filter knows, by `AUDIT_ARCH_*`.
#[cfg(target_arch = "x86_64")]
const ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const ARCH: u32 = 0xC000_00B7;

// The kernel's BPF, as `linux/filter.h` and `linux/bpf_common.h` spell it: each the class, the
// operation and the source added up (`BPF_LD | BPF_W | BPF_ABS` is 0x00 + 0x00 + 0x20), written
// as the sum, an OR with nothing in it being refused by the warnings' check.
const LD_W_ABS: u16 = 0x20;
const JMP_JEQ_K: u16 = 0x15;
const JMP_JSET_K: u16 = 0x45;
const RET_K: u16 = 0x06;

const RET_ALLOW: u32 = 0x7fff_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const RET_KILL_PROCESS: u32 = 0x8000_0000;

/// Where `struct seccomp_data` keeps the call's number, its architecture and its arguments
/// (the low half of each, on these little-endian machines).
const NR: u32 = 0;
const ARCH_AT: u32 = 4;
const fn arg(n: u32) -> u32 {
    16 + 8 * n
}

/// One instruction.
fn op(code: u16, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

/// The calls refused outright.
fn refused() -> Vec<libc::c_long> {
    #[cfg_attr(not(target_arch = "x86_64"), allow(unused_mut))]
    let mut calls = vec![
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_kill,
        libc::SYS_unlinkat,
        libc::SYS_renameat,
        libc::SYS_renameat2,
        libc::SYS_mkdirat,
        libc::SYS_linkat,
        libc::SYS_symlinkat,
        libc::SYS_fchmodat,
        libc::SYS_fchmod,
        libc::SYS_fchownat,
        libc::SYS_fchown,
        libc::SYS_truncate,
        libc::SYS_mknodat,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_setns,
        libc::SYS_unshare,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_name_to_handle_at,
        libc::SYS_open_by_handle_at,
        libc::SYS_openat2,
    ];
    #[cfg(target_arch = "x86_64")]
    calls.extend([
        libc::SYS_fork,
        libc::SYS_vfork,
        libc::SYS_creat,
        libc::SYS_unlink,
        libc::SYS_rename,
        libc::SYS_mkdir,
        libc::SYS_rmdir,
        libc::SYS_link,
        libc::SYS_symlink,
        libc::SYS_chmod,
        libc::SYS_chown,
        libc::SYS_lchown,
        libc::SYS_mknod,
    ]);
    calls
}

/// The calls that open a file, with which argument holds their flags: refused where the flags
/// ask to write, make, add to or cut the file.
fn opens() -> Vec<(libc::c_long, u32)> {
    #[cfg_attr(not(target_arch = "x86_64"), allow(unused_mut))]
    let mut calls = vec![(libc::SYS_openat, 2)];
    #[cfg(target_arch = "x86_64")]
    calls.push((libc::SYS_open, 1));
    calls
}

/// The flags that write.
fn writing() -> u32 {
    (libc::O_WRONLY | libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC | libc::O_APPEND) as u32
}

/// The filter, instruction by instruction.
pub(super) fn program() -> Vec<libc::sock_filter> {
    let errno = RET_ERRNO | libc::EPERM as u32;
    #[cfg_attr(not(target_arch = "x86_64"), allow(unused_mut))]
    let mut p = vec![
        // Another architecture's numbering: the process ends.
        op(LD_W_ABS, 0, 0, ARCH_AT),
        op(JMP_JEQ_K, 1, 0, ARCH),
        op(RET_K, 0, 0, RET_KILL_PROCESS),
        op(LD_W_ABS, 0, 0, NR),
    ];
    #[cfg(target_arch = "x86_64")]
    p.extend([
        // x32's calls carry this bit.
        op(JMP_JSET_K, 0, 1, 0x4000_0000),
        op(RET_K, 0, 0, RET_KILL_PROCESS),
    ]);
    for call in refused() {
        p.push(op(JMP_JEQ_K, 0, 1, call as u32));
        p.push(op(RET_K, 0, 0, errno));
    }
    for (call, flags_at) in opens() {
        p.extend([
            op(JMP_JEQ_K, 0, 3, call as u32),
            op(LD_W_ABS, 0, 0, arg(flags_at)),
            op(JMP_JSET_K, 0, 1, writing()),
            op(RET_K, 0, 0, errno),
            op(LD_W_ABS, 0, 0, NR),
        ]);
    }
    // A process made, not a thread: refused.
    p.extend([
        op(JMP_JEQ_K, 0, 4, libc::SYS_clone as u32),
        op(LD_W_ABS, 0, 0, arg(0)),
        op(JMP_JSET_K, 1, 0, libc::CLONE_THREAD as u32),
        op(RET_K, 0, 0, errno),
        op(LD_W_ABS, 0, 0, NR),
        op(JMP_JEQ_K, 0, 1, libc::SYS_clone3 as u32),
        op(RET_K, 0, 0, RET_ERRNO | libc::ENOSYS as u32),
        op(RET_K, 0, 0, RET_ALLOW),
    ]);
    p
}

pub(super) fn enter() -> Result<(), String> {
    let mut filter = program();
    let program = libc::sock_fprog {
        len: u16::try_from(filter.len()).map_err(|_| "the filter is too long".to_string())?,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: documented calls with a filter that lives across them; `no_new_privs` is what lets
    // a process without privileges set a filter, and is itself a wall (no set-uid program).
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(format!(
                "Linux would not hold the JavaScript host to its rights: {}",
                std::io::Error::last_os_error()
            ));
        }
        let set = libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            libc::SECCOMP_FILTER_FLAG_TSYNC,
            std::ptr::addr_of!(program),
        );
        if set != 0 {
            let why = std::io::Error::last_os_error();
            // A kernel built without seccomp: nothing here to enter.
            if matches!(why.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
                return Ok(());
            }
            return Err(format!(
                "Linux's seccomp would not take the JavaScript host: {why}"
            ));
        }
    }
    Ok(())
}
