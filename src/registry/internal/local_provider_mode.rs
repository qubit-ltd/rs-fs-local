// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Authority modes supported by the local filesystem provider.

use std::path::PathBuf;

use qubit_fs::FileSystemId;

/// Selects host-wide or descriptor-rooted authority for a provider instance.
#[derive(Clone, Debug)]
#[must_use]
pub(in crate::registry) enum LocalProviderMode {
    /// Resolves paths against the process host filesystem.
    Host,
    /// Resolves paths below one retained native directory.
    Rooted {
        /// Stable identity exposed by the rooted filesystem.
        id: FileSystemId,
        /// Native directory retained as the filesystem authority.
        root: PathBuf,
    },
}
