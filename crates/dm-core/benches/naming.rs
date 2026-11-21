// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Naming, at 100, 1,000 and 10,000 names: a server's name made safe (`sanitize_name`), the
//! name a response gives a file (`filename_for`), and the free name each waiting download will
//! get in a folder where half the names are taken, asking the disk for each
//! (`nth_free_name`) and with the folder listed once (`nth_free_name_with`, gaps row 210).
//! Run by hand, never in CI (docs/testing.md, Benchmarks; gaps row 190):
//! `cargo bench -p dm-core --bench naming`.

use std::collections::HashSet;
use std::hint::black_box;
use std::path::Path;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

const SIZES: [usize; 3] = [100, 1_000, 10_000];
const EXTENSIONS: [&str; 5] = ["zip", "mp4", "pdf", "bin", "mp3"];

/// What servers send: folder parts, characters a disk refuses, direction controls, spaces.
fn raw_names(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let ext = EXTENSIONS[i % EXTENSIONS.len()];
            match i % 4 {
                0 => format!("report {i}.{ext}"),
                1 => format!("../../folder/sub\\file:{i}?.{ext}"),
                2 => format!("invoice\u{202e}{i}<draft>.{ext}"),
                _ => format!("  CON {i} \"quoted\" |piped|.{ext}  "),
            }
        })
        .collect()
}

/// A response's disposition (every other one has none), address and content type.
fn responses(n: usize) -> Vec<(Option<String>, String, Option<&'static str>)> {
    (0..n)
        .map(|i| {
            let ext = EXTENSIONS[i % EXTENSIONS.len()];
            let disposition = (i % 2 == 0).then(|| {
                format!("attachment; filename*=UTF-8''r%C3%A9sum%C3%A9%20{i}.{ext}; filename=\"r {i}.{ext}\"")
            });
            let url = format!("http://127.0.0.1:9/files/{i}/the%20file%20{i}.{ext}?token={i}&x=1");
            let mime = (i % 3 == 0).then_some("application/octet-stream");
            (disposition, url, mime)
        })
        .collect()
}

fn sanitize(c: &mut Criterion) {
    let mut group = c.benchmark_group("sanitize_name");
    for n in SIZES {
        let names = raw_names(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &names, |b, names| {
            b.iter(|| {
                for name in names {
                    black_box(dm_core::sanitize_name(black_box(name)));
                }
            });
        });
    }
    group.finish();
}

fn filename_for(c: &mut Criterion) {
    let mut group = c.benchmark_group("filename_for");
    for n in SIZES {
        let responses = responses(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &responses,
            |b, responses| {
                b.iter(|| {
                    for (disposition, url, mime) in responses {
                        black_box(dm_core::filename_for(
                            black_box(disposition.as_deref()),
                            black_box(url),
                            black_box(*mime),
                        ));
                    }
                });
            },
        );
    }
    group.finish();
}

/// A folder holding every other one of `n` names, and the names.
fn folder_half_taken(n: usize) -> (tempfile::TempDir, Vec<String>, HashSet<String>) {
    let dir = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..n)
        .map(|i| format!("file {i}.{}", EXTENSIONS[i % EXTENSIONS.len()]))
        .collect();
    for name in names.iter().step_by(2) {
        std::fs::write(dir.path().join(name), b"").unwrap();
    }
    // The listing as dm-shell keeps it on Windows: each name lowercased.
    let listed = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_lowercase())
        .collect();
    (dir, names, listed)
}

fn free_names(c: &mut Criterion) {
    let mut group = c.benchmark_group("free_name");
    for n in SIZES {
        let (dir, names, listed) = folder_half_taken(n);
        let folder: &Path = dir.path();
        group.throughput(Throughput::Elements(n as u64));
        if n >= 10_000 {
            // A pass asks the disk 15,000 times: a second or so on Windows.
            group.sample_size(10);
        }
        group.bench_with_input(BenchmarkId::new("disk", n), &names, |b, names| {
            b.iter(|| {
                for name in names {
                    black_box(dm_core::nth_free_name(folder, black_box(name), 0));
                }
            });
        });
        group.bench_with_input(BenchmarkId::new("listed", n), &names, |b, names| {
            b.iter(|| {
                for name in names {
                    black_box(dm_core::nth_free_name_with(
                        folder,
                        black_box(name),
                        0,
                        |path| {
                            path.file_name().is_some_and(|n| {
                                listed.contains(&n.to_string_lossy().to_lowercase())
                            })
                        },
                    ));
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, sanitize, filename_for, free_names);
criterion_main!(benches);
