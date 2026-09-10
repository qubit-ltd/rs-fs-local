// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Byte windows on an already-open native regular file.

use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Take;

use qubit_local_files::LocalFileReader;

/// Retains one native handle and limits consumption without reopening its path.
pub(crate) struct LocalRangeReader {
    /// Native reader with a decreasing byte allowance.
    inner: Take<LocalFileReader>,
}

impl LocalRangeReader {
    /// Positions an opened file and limits reads to the requested window.
    ///
    /// Empty windows retain the opened handle without seeking. Offsets beyond
    /// the signed native file-position range are empty only when the same
    /// handle's metadata proves the offset is beyond EOF. Other nonzero offsets
    /// seek normally and preserve any native I/O error. Metadata is an opening
    /// observation, not a file snapshot or a promise against concurrent
    /// changes.
    pub(crate) fn new(mut reader: LocalFileReader, offset: u64, length: Option<u64>) -> io::Result<Self> {
        let beyond_native_range = offset > i64::MAX as u64 && offset >= reader.metadata().len();
        let length = if length == Some(0) || beyond_native_range {
            0
        } else {
            if offset != 0 {
                reader.seek(SeekFrom::Start(offset))?;
            }
            length.unwrap_or(u64::MAX)
        };
        Ok(Self {
            inner: reader.take(length),
        })
    }
}

impl Read for LocalRangeReader {
    /// Consumes at most the remaining window, propagating native read errors.
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.inner.read(output)
    }
}
