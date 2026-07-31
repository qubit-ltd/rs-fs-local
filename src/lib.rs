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
pub use local_file_systems::host_path_to_logical;
#[cfg(feature = "registry")]
pub use registry::LocalFileSystemProvider;
