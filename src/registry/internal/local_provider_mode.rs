// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Authority modes supported by the local filesystem provider.

use std::fmt;

use qubit_fs::FileSystem;

/// Selects host-wide or descriptor-rooted authority for a provider instance.
#[derive(Clone)]
#[must_use]
pub(in crate::registry) enum LocalProviderMode {
    /// Resolves paths against the process host filesystem.
    Host,
    /// Resolves paths below one retained native directory.
    Rooted {
        /// Opened facade retaining the descriptor-backed authority.
        file_system: FileSystem,
    },
}

impl fmt::Debug for LocalProviderMode {
    /// Formats the mode without expanding the internal filesystem SPI.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host => formatter.write_str("Host"),
            Self::Rooted { file_system } => formatter
                .debug_struct("Rooted")
                .field("file_system_id", file_system.properties().info().id())
                .finish(),
        }
    }
}
