// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::hint::black_box;

use criterion::BatchSize;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use qubit_fs::CopyOptions;
use qubit_fs::ReadOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::host_path_to_logical;
use tempfile::tempdir;

fn bench_local_facade_read_prefix(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let native_path = directory.path().join("payload");
    fs::write(&native_path, vec![0x7f_u8; 1 << 20])
        .expect("benchmark payload should be written");
    let logical_path =
        host_path_to_logical(&native_path).expect("logical path should map");
    let filesystem =
        LocalFileSystems::host().expect("local facade should construct");
    let mut group = c.benchmark_group("local_facade_read_prefix");
    for max_bytes in [4 * 1024, 64 * 1024, 1 << 20] {
        group.throughput(Throughput::Bytes(max_bytes as u64));
        group.bench_function(format!("max_{max_bytes}"), |bench| {
            bench.iter(|| {
                let bytes = filesystem
                    .read_prefix(
                        black_box(&logical_path),
                        ReadOptions::default(),
                        max_bytes,
                    )
                    .expect("facade prefix read should succeed");
                black_box(bytes.len());
            });
        });
    }
    group.finish();
}

fn bench_local_facade_copy(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let native_source = directory.path().join("source");
    fs::write(&native_source, vec![0x4a_u8; 1 << 20])
        .expect("benchmark source should be written");
    let native_target = directory.path().join("target");
    let source =
        host_path_to_logical(&native_source).expect("source should map");
    let target =
        host_path_to_logical(&native_target).expect("target should map");
    let filesystem =
        LocalFileSystems::host().expect("local facade should construct");
    c.bench_function("local_facade_copy", |bench| {
        bench.iter_batched(
            || {
                let _ = fs::remove_file(&native_target);
            },
            |_| {
                let outcome = filesystem
                    .copy(
                        black_box(&source),
                        black_box(&target),
                        CopyOptions::default(),
                    )
                    .expect("facade copy should succeed");
                black_box(outcome.stats().bytes);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_local_facade_read_prefix,
    bench_local_facade_copy
);
criterion_main!(benches);
