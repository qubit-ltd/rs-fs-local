// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local filesystem provider for [`qubit_fs`].

#![deny(missing_docs)]

mod internal;
mod local_file_system;
#[cfg(feature = "registry")]
mod local_file_system_provider;
mod rooted_local_file_system;

pub use local_file_system::LocalFileSystem;
#[cfg(feature = "registry")]
pub use local_file_system_provider::LocalFileSystemProvider;
pub use rooted_local_file_system::RootedLocalFileSystem;
