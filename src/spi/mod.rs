// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public synchronous local filesystem SPI implementations.

mod error_mapper;
mod internal;
mod local_directory_stream_spi;
mod local_file_system_spi;
mod local_file_writer_spi;
mod local_options_mapper;
mod local_outcome_mapper;
mod local_range_reader;
mod local_temp_path_projection;
mod local_temp_resource_spi;

pub use local_file_system_spi::LocalFileSystemSpi;
