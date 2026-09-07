// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![no_main]

use libfuzzer_sys::fuzz_target;
use qubit_fs::path::ConnectionUri;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_registry::FileSystemConfig;
use qubit_fs_registry::FileSystemRegistry;

fuzz_target!(|input: &[u8]| {
    let input = &input[..input.len().min(4096)];
    let suffix = String::from_utf8_lossy(input);
    let text = format!("file:///{suffix}");

    let Ok(uri) = ConnectionUri::parse(&text) else {
        return;
    };

    let registry = FileSystemRegistry::default();
    if registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .is_err()
    {
        return;
    }

    let first = match registry.resolve_config(&FileSystemConfig::new(uri)) {
        Ok(resolution) => resolution,
        Err(error) => {
            let _ = format!("{error}");
            return;
        }
    };

    assert_eq!(first.canonical_uri().scheme(), "file");

    let Ok(replayed_uri) = ConnectionUri::parse(first.canonical_uri().as_str())
    else {
        panic!("a canonical URI must be parseable");
    };
    let second = registry
        .resolve_config(&FileSystemConfig::new(replayed_uri))
        .expect("a canonical URI must resolve");
    assert_eq!(first.canonical_uri(), second.canonical_uri());
});
