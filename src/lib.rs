// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local filesystem provider for [`qubit_fs`].

#![deny(missing_docs)]

mod local_file_system;

pub use local_file_system::LocalFileSystem;
