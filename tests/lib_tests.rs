// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs_local::{
    LocalFileSystem,
    LocalFileSystemProvider,
};

/// Confirms the synchronous local filesystem is part of the crate's public API.
#[test]
fn test_local_file_system_is_exported() {
    let _ = std::any::TypeId::of::<LocalFileSystem>();
}

/// Confirms the local filesystem provider is part of the crate's public API.
#[test]
fn test_local_file_system_provider_is_exported() {
    let _ = std::any::TypeId::of::<LocalFileSystemProvider>();
}
