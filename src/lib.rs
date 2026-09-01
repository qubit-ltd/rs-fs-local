// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thin local-files adapter for [`qubit_fs`].

#![deny(missing_docs)]

mod constants;
mod local_file_systems;
mod local_resource_policy;
mod path;
#[cfg(feature = "registry")]
mod registry;
pub mod spi;

pub use local_file_systems::LocalFileSystems;
pub use local_file_systems::host_path_to_logical;
pub use local_resource_policy::LocalCopyResourceLimits;
pub use local_resource_policy::LocalListResourceLimits;
pub use local_resource_policy::LocalResourcePolicy;
#[cfg(feature = "registry")]
pub use registry::LocalFileSystemProvider;
