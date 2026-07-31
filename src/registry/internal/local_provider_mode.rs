// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Authority modes supported by the local filesystem provider.

use qubit_fs::FileSystem;

/// Selects host-wide or descriptor-rooted authority for a provider instance.
#[derive(Clone)]
#[must_use]
pub(in crate::registry) enum LocalProviderMode {
    /// Resolves paths against the process host filesystem.
    Host,
    /// Resolves paths below one retained native directory.
    Rooted {
        /// Filesystem retaining the descriptor-backed native authority.
        file_system: FileSystem,
    },
}
