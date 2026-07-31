// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thin local-files adapter for [`qubit_fs`].

#![deny(missing_docs)]

mod local_file_systems;
mod path;
#[cfg(feature = "registry")]
mod registry;
pub mod spi;

pub use local_file_systems::LocalFileSystems;
/// Converts an absolute host-native path to a canonical logical path.
///
/// This conversion preserves platform-native path components, including
/// non-UTF-8 Unix names, without routing through lossy display text.
pub fn host_path_to_logical(
    path: &std::path::Path,
) -> qubit_fs::FsResult<qubit_fs::Path> {
    path::local_path_mapper::host_logical(path)
}
#[cfg(feature = "registry")]
pub use registry::LocalFileSystemProvider;
