// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::hint::black_box;
use std::time::Duration;

use criterion::BatchSize;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use qubit_fs::copy::CopyOptions;
use qubit_fs::directory::ListFilter;
use qubit_fs::directory::ListOptions;
use qubit_fs::directory::ListScope;
use qubit_fs::error::FsErrorKind;
use qubit_fs::path::Path;
use qubit_fs::read::ReadOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use tempfile::tempdir;

fn sample_offsets(sample_count: usize, sample_size: usize, file_size: usize) -> Vec<u64> {
    let anchors = [
        0,
        file_size / 2,
        file_size - sample_size,
        file_size / 4,
        file_size * 3 / 4,
    ];
    (0..sample_count)
        .map(|index| anchors[(index * 3) % anchors.len()] as u64)
        .collect()
}

fn read_samples(reader: &mut qubit_fs::read::RandomAccessReader, offsets: &[u64], sample_size: usize) -> usize {
    let mut total = 0;
    let mut buffer = vec![0_u8; sample_size];
    for offset in offsets {
        let read = reader
            .read_at(black_box(*offset), black_box(&mut buffer))
            .expect("random access sample should succeed");
        total += read;
        black_box((read, &buffer));
    }
    total
}

fn bench_local_facade_random_access(c: &mut Criterion) {
    const FILE_SIZE: usize = 1 << 20;
    let directory = tempdir().expect("benchmark directory should be created");
    let native_path = directory.path().join("random-payload");
    let expected: Vec<u8> = (0..FILE_SIZE).map(|index| (index % 251) as u8).collect();
    fs::write(&native_path, &expected).expect("random access payload should be written");
    let logical_path = host_path_to_logical(&native_path).expect("logical path should map");
    let host = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host facade should construct");
    let rooted = LocalFileSystems::rooted(directory.path(), LocalResourcePolicy::unbounded())
        .expect("rooted facade should construct");
    let rooted_path = Path::parse("/random-payload").expect("rooted path should parse");
    let mut group = c.benchmark_group("local_facade_random_access");

    for (mode, filesystem, path) in [("host", &host, &logical_path), ("rooted", &rooted, &rooted_path)] {
        for sample_size in [512, 4096] {
            for sample_count in [1, 8, 32] {
                let offsets = sample_offsets(sample_count, sample_size, FILE_SIZE);
                // Validate all positions and expected bytes outside the timed
                // regions.
                let mut check_reader = filesystem
                    .open_random_access_reader(path, ReadOptions::default())
                    .expect("random access reader should open");
                for offset in &offsets {
                    let mut actual = vec![0; sample_size];
                    let read = check_reader.read_at(*offset, &mut actual).expect("sample should read");
                    assert_eq!(read, sample_size);
                    assert_eq!(actual, expected[*offset as usize..*offset as usize + sample_size]);
                }

                let bytes = (sample_size * sample_count) as u64;
                group.throughput(Throughput::Bytes(bytes));
                group.bench_function(
                    format!("{mode}/open_and_sample/{sample_count}x{sample_size}"),
                    |bench| {
                        bench.iter(|| {
                            let mut reader = filesystem
                                .open_random_access_reader(black_box(path), ReadOptions::default())
                                .expect("random access reader should open");
                            black_box(read_samples(&mut reader, &offsets, sample_size));
                        });
                    },
                );

                let mut reused = filesystem
                    .open_random_access_reader(path, ReadOptions::default())
                    .expect("reused random access reader should open");
                group.bench_function(format!("{mode}/reuse_session/{sample_count}x{sample_size}"), |bench| {
                    bench.iter(|| black_box(read_samples(&mut reused, &offsets, sample_size)));
                });
            }
        }
    }
    group.finish();
}

fn bench_local_facade_filtered_list(c: &mut Criterion) {
    let mut group = c.benchmark_group("local_facade_filtered_list");
    for unrelated_descendants in [0, 128, 1024] {
        let directory = tempdir().expect("benchmark directory should be created");
        let wanted = directory.path().join("wanted");
        fs::create_dir_all(wanted.join("nested")).expect("wanted tree should be created");
        fs::write(wanted.join("nested/leaf"), b"wanted").expect("wanted file should be written");
        for index in 0..unrelated_descendants {
            let sibling = directory.path().join(format!("other-{index:04}"));
            fs::create_dir_all(sibling.join("deep")).expect("unrelated subtree should be created");
            fs::write(sibling.join("deep/leaf"), b"unrelated").expect("unrelated file should be written");
        }
        // Leave room for observing every root sibling and the wanted tree;
        // unfiltered descent through the unrelated descendants exceeds this.
        let provider_entry_budget = 1032;
        let list_limits = LocalListResourceLimits::new(8, provider_entry_budget, 1 << 20, 16, Duration::from_secs(30))
            .expect("finite provider list limits should be valid");
        let policy = LocalResourcePolicy::standard().with_list_limits(Some(list_limits));
        let filesystem = LocalFileSystems::rooted(directory.path(), policy).expect("rooted facade should construct");
        let root = Path::parse("/").expect("rooted listing path should parse");
        let options = ListOptions::default()
            .with_recursive(true)
            .with_filter(Some(ListFilter::Subtree("wanted".to_owned())))
            .with_max_entries(Some(16))
            .with_max_depth(Some(8))
            .with_deadline(Some(Duration::from_secs(30)));
        let scope = ListScope::Path(root);
        let mut check_stream = filesystem
            .list(&scope, options.clone())
            .expect("filtered listing should open");
        let mut expected_entries = Vec::new();
        while let Some(entry) = check_stream.next_entry().expect("filtered entry should read") {
            expected_entries.push(entry.path.as_str().to_owned());
        }
        expected_entries.sort();
        assert_eq!(expected_entries.len(), 3);
        assert!(
            expected_entries
                .iter()
                .all(|path| path == "/wanted" || path.starts_with("/wanted/"))
        );
        if unrelated_descendants == 1024 {
            let unfiltered_options = ListOptions::default()
                .with_recursive(true)
                .with_max_entries(Some(4096))
                .with_max_depth(Some(8))
                .with_deadline(Some(Duration::from_secs(30)));
            let mut unfiltered = filesystem
                .list(&scope, unfiltered_options)
                .expect("unfiltered evidence listing should open");
            let mut error = None;
            loop {
                match unfiltered.next_entry() {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(failure) => {
                        error = Some(failure);
                        break;
                    }
                }
            }
            assert_eq!(
                error
                    .expect("unfiltered traversal should exceed provider entry budget")
                    .kind(),
                FsErrorKind::ResourceLimitExceeded
            );
        }
        group.throughput(Throughput::Elements(3));
        group.bench_function(format!("unrelated_descendants_{unrelated_descendants}"), |bench| {
            bench.iter(|| {
                let mut stream = filesystem
                    .list(black_box(&scope), options.clone())
                    .expect("filtered listing should open");
                let mut entries = Vec::new();
                while let Some(entry) = stream.next_entry().expect("filtered entry should read") {
                    entries.push(entry.path.as_str().to_owned());
                }
                entries.sort();
                black_box(entries);
            });
        });
    }
    group.finish();
}

fn bench_local_facade_read_prefix(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let native_path = directory.path().join("payload");
    fs::write(&native_path, vec![0x7f_u8; 1 << 20]).expect("benchmark payload should be written");
    let logical_path = host_path_to_logical(&native_path).expect("logical path should map");
    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("local facade should construct");
    let mut group = c.benchmark_group("local_facade_read_prefix");
    for max_bytes in [4 * 1024, 64 * 1024, 1 << 20] {
        group.throughput(Throughput::Bytes(max_bytes as u64));
        group.bench_function(format!("max_{max_bytes}"), |bench| {
            bench.iter(|| {
                let outcome = filesystem
                    .read_prefix(black_box(&logical_path), ReadOptions::default(), max_bytes)
                    .expect("facade prefix read should succeed");
                black_box(outcome.bytes().len());
            });
        });
    }
    group.finish();
}

fn bench_local_facade_copy(c: &mut Criterion) {
    let directory = tempdir().expect("benchmark directory should be created");
    let native_source = directory.path().join("source");
    fs::write(&native_source, vec![0x4a_u8; 1 << 20]).expect("benchmark source should be written");
    let native_target = directory.path().join("target");
    let source = host_path_to_logical(&native_source).expect("source should map");
    let target = host_path_to_logical(&native_target).expect("target should map");
    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("local facade should construct");
    c.bench_function("local_facade_copy", |bench| {
        bench.iter_batched(
            || {
                let _ = fs::remove_file(&native_target);
            },
            |_| {
                let outcome = filesystem
                    .copy(black_box(&source), black_box(&target), CopyOptions::default())
                    .expect("facade copy should succeed");
                black_box(outcome.stats().bytes);
            },
            BatchSize::PerIteration,
        );
    });
}

fn bench_local_facade_list(c: &mut Criterion) {
    let mut group = c.benchmark_group("local_facade_list");
    for entry_count in [1_000, 10_000] {
        let directory = tempdir().expect("benchmark directory should be created");
        for index in 0..entry_count {
            fs::write(directory.path().join(format!("entry-{index:05}")), b"x")
                .expect("benchmark entry should be written");
        }
        let host = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host facade should construct");
        let host_root = host_path_to_logical(directory.path()).expect("host directory should map");
        let rooted = LocalFileSystems::rooted(directory.path(), LocalResourcePolicy::unbounded())
            .expect("rooted facade should construct");
        let rooted_root = Path::parse("/").expect("absolute rooted listing should parse");
        for (mode, filesystem, root) in [("host", &host, &host_root), ("rooted_absolute", &rooted, &rooted_root)] {
            group.throughput(Throughput::Elements(entry_count as u64));
            group.bench_function(format!("{mode}/{entry_count}"), |bench| {
                bench.iter(|| {
                    let scope = ListScope::Path(root.clone());
                    let mut stream = filesystem
                        .list(black_box(&scope), ListOptions::default())
                        .expect("benchmark listing should open");
                    let mut count = 0;
                    while stream.next_entry().expect("benchmark entry should read").is_some() {
                        count += 1;
                    }
                    assert_eq!(count, entry_count);
                    black_box(count);
                });
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_local_facade_read_prefix,
    bench_local_facade_copy,
    bench_local_facade_list,
    bench_local_facade_random_access,
    bench_local_facade_filtered_list
);
criterion_main!(benches);
