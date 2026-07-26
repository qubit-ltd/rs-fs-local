// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private local provider implementation types.

mod local_file_write_session;
mod rooted_file_write_session;

pub(crate) use local_file_write_session::LocalFileWriteSession;
pub(crate) use rooted_file_write_session::RootedFileWriteSession;
