// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Random reads through one already-open native file handle.

use std::io::Seek;
use std::io::SeekFrom;

use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::path::Path;
use qubit_fs::spi::RandomAccessReadSession;
use qubit_io::Input;
use qubit_local_files as native_files;

use super::error_mapper;

/// Adapts one native reader to the portable random-access session contract.
pub(crate) struct LocalRandomAccessReader {
    reader: native_files::LocalFileReader,
    path: Path,
    provider_id: String,
}

impl LocalRandomAccessReader {
    /// Retains the opened native reader and its portable error context.
    pub(crate) fn new(reader: native_files::LocalFileReader, path: Path, provider_id: String) -> Self {
        Self { reader, path, provider_id }
    }

    fn map_io(&self, error: std::io::Error) -> qubit_fs::error::FsError {
        let native_error = native_files::error::LocalFileError::from_io(
            native_files::error::LocalFileOperation::Read,
            None,
            None,
            error,
        );
        error_mapper::map(native_error, FsOperation::Read, &self.path, None, &self.provider_id)
    }
}

impl RandomAccessReadSession for LocalRandomAccessReader {
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> FsResult<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.reader
            .seek(SeekFrom::Start(offset))
            .map_err(|error| self.map_io(error))?;
        Input::read(&mut self.reader, output).map_err(|error| self.map_io(error))
    }
}
