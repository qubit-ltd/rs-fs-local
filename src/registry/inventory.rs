// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Submit the bounded host provider to the synchronous filesystem inventory.

use qubit_fs_registry::FileSystemSpec;
use qubit_fs_registry::sync_file_system_providers::Entry;
use qubit_spi::submit_sync_provider;

submit_sync_provider! {
    inventory_entry = Entry;
    spec = FileSystemSpec;
    provider = super::LocalFileSystemProvider::host(crate::LocalResourcePolicy::standard());
}
