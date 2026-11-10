// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thin local-files adapter for [`qubit_fs`].
//!
//! # Choose a filesystem namespace
//!
//! [`LocalFileSystems::host`] accesses the process host namespace and accepts
//! absolute logical paths. [`LocalFileSystems::rooted`] opens a native root
//! with a process-local identity; [`LocalFileSystems::rooted_with_id`] uses an
//! identity supplied by the caller. Rooted paths are resolved from the
//! virtual working directory, which starts at `/`.
//!
//! # Convert host paths
//!
//! Convert native host paths with [`host_path_to_logical`] before passing them
//! to a filesystem facade. This preserves native path representation; avoid
//! lossy string conversion.
//!
//! # Resource policy
//!
//! Each facade constructor takes an explicit [`LocalResourcePolicy`]. Its
//! [`standard` policy](LocalResourcePolicy::standard) sets finite per-operation
//! budgets. These are not a shared concurrency quota and do not automatically
//! limit temporary-directory cleanup.
//!
//! # Quick start
//!
//! ```
//! use qubit_fs_local::{LocalFileSystems, LocalResourcePolicy};
//!
//! let fs = LocalFileSystems::host(LocalResourcePolicy::standard())?;
//! assert_eq!(fs.properties().info().id().as_str(), "local-host");
//! # Ok::<(), qubit_fs::error::FsError>(())
//! ```
//!
//! # Optional registry integration
//!
//! Enable the `registry` feature to include the provider and inventory
//! integration. See the [user guide](https://github.com/qubit-ltd/rs-fs-local/blob/main/doc/user_guide.md)
//! for registry setup and provider selection.

#![deny(missing_docs)]

mod constants;
mod local_copy_resource_limits;
mod local_delete_resource_limits;
mod local_directory_reopen_policy;
mod local_file_systems;
mod local_list_resource_limits;
mod local_resource_policy;
mod local_temp_cleanup_control;
mod local_temp_cleanup_resource_limits;
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
pub use local_temp_cleanup_control::LocalTempCleanupControl;
pub use local_temp_cleanup_resource_limits::LocalTempCleanupResourceLimits;
#[cfg(feature = "registry")]
pub use registry::LocalFileSystemProvider;
