// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![no_main]

use libfuzzer_sys::fuzz_target;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::ConnectionUri;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_registry::FileSystemConfig;
use qubit_fs_registry::FileSystemRegistry;

fn check_replay(registry: &FileSystemRegistry, text: &str) {
    let Ok(uri) = ConnectionUri::parse(text) else {
        return;
    };
    let Ok(first) = registry.resolve_config(&FileSystemConfig::new(uri)) else {
        return;
    };

    assert_eq!(first.canonical_uri().scheme(), "file");
    assert_eq!(first.file_system().properties().info().provider_id(), "local-file");
    let replayed_uri = ConnectionUri::parse(first.canonical_uri().as_str()).expect("canonical URI must be parseable");
    let second = registry
        .resolve_config(&FileSystemConfig::new(replayed_uri))
        .expect("canonical URI must replay through its provider");
    assert_eq!(first.path(), second.path());
    assert_eq!(first.canonical_uri(), second.canonical_uri());
    assert_eq!(
        first.file_system().properties().info().provider_id(),
        second.file_system().properties().info().provider_id()
    );
}

fn check_success_replay(registry: &FileSystemRegistry, text: &str, expected_uri: &str, expected_path: &str) {
    let uri = ConnectionUri::parse(text).expect("success seed URI must parse");
    let first = registry
        .resolve_config(&FileSystemConfig::new(uri))
        .expect("success seed URI must resolve");
    assert_eq!(first.canonical_uri().as_str(), expected_uri);
    assert_eq!(first.path().as_str(), expected_path);
    assert_eq!(first.file_system().properties().info().provider_id(), "local-file");
    check_replay(registry, text);
}

fuzz_target!(|input: &[u8]| {
    let input = &input[..input.len().min(4096)];
    let suffix = String::from_utf8_lossy(input);
    let host_registry = FileSystemRegistry::default();
    host_registry
        .register(LocalFileSystemProvider::host(LocalResourcePolicy::unbounded()))
        .expect("Host provider registration must succeed");
    #[cfg(windows)]
    let host_text = format!("file:///C:/fuzz/{suffix}");
    #[cfg(not(windows))]
    let host_text = format!("file:///fuzz/{suffix}");
    check_replay(&host_registry, &host_text);

    let rooted_fixture = tempfile::tempdir().expect("Rooted authority fixture must be created");
    let rooted_registry = FileSystemRegistry::default();
    rooted_registry
        .register(
            LocalFileSystemProvider::rooted(
                FileSystemId::new("fuzz-rooted-filesystem").expect("filesystem ID"),
                rooted_fixture.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("Rooted provider must open its fixture authority"),
        )
        .expect("Rooted provider registration must succeed");
    let rooted_text = format!("file:///rooted/{suffix}");
    check_replay(&rooted_registry, &rooted_text);

    #[cfg(windows)]
    let host_success = "file:///C:/reports/summary.csv";
    #[cfg(not(windows))]
    let host_success = "file:///reports/summary.csv";
    #[cfg(windows)]
    check_success_replay(&host_registry, host_success, host_success, "/C:/reports/summary.csv");
    #[cfg(not(windows))]
    check_success_replay(&host_registry, host_success, host_success, "/reports/summary.csv");
    check_success_replay(
        &rooted_registry,
        "file:///reports/summary.csv",
        "file:///reports/summary.csv",
        "/reports/summary.csv",
    );
});
