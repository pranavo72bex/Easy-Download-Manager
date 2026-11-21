// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Range planning (`dm_core::plan`): a file cut into the ranges its connections fetch at once,
//! over file sizes from one below the smallest range to a terabyte and from one connection to
//! more than the most allowed. Run by hand, never in CI (docs/testing.md, Benchmarks; gaps
//! row 190): `cargo bench -p dm-core --bench plan`.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

const MIB: u64 = 1024 * 1024;

fn ranges(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan");
    for (label, size) in [
        ("512KiB", MIB / 2),
        ("100MiB", 100 * MIB),
        ("4GiB", 4 * 1024 * MIB),
        ("1TiB", 1024 * 1024 * MIB),
    ] {
        for connections in [1usize, 8, 32, 64] {
            group.bench_with_input(
                BenchmarkId::new(label, connections),
                &(size, connections),
                |b, &(size, connections)| {
                    b.iter(|| dm_core::plan(black_box(Some(size)), black_box(connections), true));
                },
            );
        }
    }
    // A length the server never said, and one that cannot be resumed: one range each.
    group.bench_function("unknown-length", |b| {
        b.iter(|| dm_core::plan(black_box(None), black_box(8), true));
    });
    group.bench_function("not-resumable", |b| {
        b.iter(|| dm_core::plan(black_box(Some(4 * 1024 * MIB)), black_box(8), false));
    });
    group.finish();
}

criterion_group!(benches, ranges);
criterion_main!(benches);
