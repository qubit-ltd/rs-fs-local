// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared local-provider identity and URI constants.
// qubit-style: allow source-test-pair

/// Stable provider identity used by the host local filesystem.
pub(crate) const LOCAL_PROVIDER_ID: &str = "local-file";

/// URI scheme accepted by the local filesystem provider.
pub(crate) const FILE_SCHEME: &str = "file";
