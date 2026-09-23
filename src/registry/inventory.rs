// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Submit the bounded host provider to the synchronous filesystem inventory.

qubit_spi::submit_sync_provider! {
    inventory_entry = qubit_fs_registry::sync_file_system_providers::Entry;
    spec = qubit_fs_registry::FileSystemSpec;
    provider = super::LocalFileSystemProvider::host(crate::LocalResourcePolicy::standard());
}
