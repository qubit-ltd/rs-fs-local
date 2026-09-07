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
mod local_copy_resource_limits;
mod local_delete_resource_limits;
mod local_directory_reopen_policy;
mod local_file_systems;
mod local_list_resource_limits;
mod local_resource_policy;
mod path;
#[cfg(feature = "registry")]
mod registry;
pub mod spi;

pub use local_copy_resource_limits::LocalCopyResourceLimits;
pub use local_delete_resource_limits::LocalDeleteResourceLimits;
pub use local_directory_reopen_policy::LocalDirectoryReopenPolicy;
pub use local_file_systems::LocalFileSystems;
pub use local_file_systems::host_path_to_logical;
pub use local_list_resource_limits::LocalListResourceLimits;
pub use local_resource_policy::LocalResourcePolicy;
#[cfg(feature = "registry")]
pub use registry::LocalFileSystemProvider;
