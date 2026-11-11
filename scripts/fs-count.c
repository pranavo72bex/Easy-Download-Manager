/*
 * Copyright (C) 2026 pranavo72bex and the dm contributors.
 * SPDX-License-Identifier: GPL-3.0-or-later
 * Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
 * with the attribution term in NOTICE. See the LICENSE and NOTICE files.
 *
 * Counts each thread's file-system calls, for a check that holds the main
 * window's draw to none (gaps row 180; `stress-check.py frame`). Preloaded
 * into a debug dm (DYLD_INSERT_LIBRARIES on macOS, LD_PRELOAD on Linux), it
 * says how many calls the calling thread has made as `dm_fs_calls`, which
 * dm's debug build reads as a draw begins and ends (dm-shell `app/costs.rs`).
 * Every call goes on to the system's own function unchanged.
 *
 * Built by the check: cc -shared -o fs-count.dylib fs-count.c (macOS),
 * cc -shared -fPIC -o fs-count.so fs-count.c -ldl (Linux).
 */

#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

static __thread unsigned long long calls;

unsigned long long dm_fs_calls(void) { return calls; }

/* The mode is there only where the file may be made. */
#define MODE_OF(flags, mode)                \
    do {                                     \
        if ((flags) & O_CREAT) {             \
            va_list rest;                    \
            va_start(rest, flags);           \
            mode = (mode_t)va_arg(rest, int); \
            va_end(rest);                    \
        }                                    \
    } while (0)

#ifdef __APPLE__
#include <sys/attr.h>
#include <sys/mount.h>

/* dyld's interposing: each pair sends the system's function to ours in every
 * other image; calls from this one still reach the system's. */
#define INTERPOSE(ours, theirs)                                                  \
    __attribute__((used)) static struct {                                        \
        const void *replacement;                                                 \
        const void *replacee;                                                    \
    } interpose_##theirs __attribute__((section("__DATA,__interpose"))) = {     \
        (const void *)(unsigned long)&ours, (const void *)(unsigned long)&theirs \
    }

static int my_stat(const char *p, struct stat *s) { calls++; return stat(p, s); }
static int my_lstat(const char *p, struct stat *s) { calls++; return lstat(p, s); }
static int my_fstatat(int d, const char *p, struct stat *s, int f) { calls++; return fstatat(d, p, s, f); }
static int my_access(const char *p, int m) { calls++; return access(p, m); }
static int my_faccessat(int d, const char *p, int m, int f) { calls++; return faccessat(d, p, m, f); }
static int my_open(const char *p, int flags, ...) {
    mode_t mode = 0;
    MODE_OF(flags, mode);
    calls++;
    return open(p, flags, mode);
}
static int my_openat(int d, const char *p, int flags, ...) {
    mode_t mode = 0;
    MODE_OF(flags, mode);
    calls++;
    return openat(d, p, flags, mode);
}
static DIR *my_opendir(const char *p) { calls++; return opendir(p); }
static char *my_realpath(const char *p, char *r) { calls++; return realpath(p, r); }
static ssize_t my_readlink(const char *p, char *b, size_t n) { calls++; return readlink(p, b, n); }
static int my_statfs(const char *p, struct statfs *s) { calls++; return statfs(p, s); }
static int my_getattrlist(const char *p, void *a, void *b, size_t n, unsigned int o) {
    calls++;
    return getattrlist(p, a, b, n, o);
}
static int my_mkdir(const char *p, mode_t m) { calls++; return mkdir(p, m); }
static int my_unlink(const char *p) { calls++; return unlink(p); }
static int my_rename(const char *a, const char *b) { calls++; return rename(a, b); }

INTERPOSE(my_stat, stat);
INTERPOSE(my_lstat, lstat);
INTERPOSE(my_fstatat, fstatat);
INTERPOSE(my_access, access);
INTERPOSE(my_faccessat, faccessat);
INTERPOSE(my_open, open);
INTERPOSE(my_openat, openat);
INTERPOSE(my_opendir, opendir);
INTERPOSE(my_realpath, realpath);
INTERPOSE(my_readlink, readlink);
INTERPOSE(my_statfs, statfs);
INTERPOSE(my_getattrlist, getattrlist);
INTERPOSE(my_mkdir, mkdir);
INTERPOSE(my_unlink, unlink);
INTERPOSE(my_rename, rename);

#else
#include <dlfcn.h>
#include <sys/vfs.h>

/* The next library's function of that name: the system's. */
#define NEXT(name) \
    static __typeof__(&name) next; \
    if (!next) next = (__typeof__(&name))dlsym(RTLD_NEXT, #name)

int stat(const char *p, struct stat *s) { NEXT(stat); calls++; return next(p, s); }
int lstat(const char *p, struct stat *s) { NEXT(lstat); calls++; return next(p, s); }
int fstatat(int d, const char *p, struct stat *s, int f) { NEXT(fstatat); calls++; return next(d, p, s, f); }
/* Rust's standard library asks with statx where the C library has it. */
int statx(int d, const char *p, int f, unsigned int m, struct statx *s) {
    NEXT(statx);
    calls++;
    return next(d, p, f, m, s);
}
int access(const char *p, int m) { NEXT(access); calls++; return next(p, m); }
int faccessat(int d, const char *p, int m, int f) { NEXT(faccessat); calls++; return next(d, p, m, f); }
int open(const char *p, int flags, ...) {
    mode_t mode = 0;
    MODE_OF(flags, mode);
    NEXT(open);
    calls++;
    return next(p, flags, mode);
}
int open64(const char *p, int flags, ...) {
    mode_t mode = 0;
    MODE_OF(flags, mode);
    NEXT(open64);
    calls++;
    return next(p, flags, mode);
}
int openat(int d, const char *p, int flags, ...) {
    mode_t mode = 0;
    MODE_OF(flags, mode);
    NEXT(openat);
    calls++;
    return next(d, p, flags, mode);
}
int openat64(int d, const char *p, int flags, ...) {
    mode_t mode = 0;
    MODE_OF(flags, mode);
    NEXT(openat64);
    calls++;
    return next(d, p, flags, mode);
}
DIR *opendir(const char *p) { NEXT(opendir); calls++; return next(p); }
char *realpath(const char *p, char *r) { NEXT(realpath); calls++; return next(p, r); }
ssize_t readlink(const char *p, char *b, size_t n) { NEXT(readlink); calls++; return next(p, b, n); }
int statfs(const char *p, struct statfs *s) { NEXT(statfs); calls++; return next(p, s); }
int mkdir(const char *p, mode_t m) { NEXT(mkdir); calls++; return next(p, m); }
int unlink(const char *p) { NEXT(unlink); calls++; return next(p); }
int rename(const char *a, const char *b) { NEXT(rename); calls++; return next(a, b); }
#endif
