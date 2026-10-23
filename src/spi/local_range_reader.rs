// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Byte windows on an already-open native regular file.

use std::io;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_io::Input;
use qubit_io::LimitInput;
use qubit_local_files::LocalFileReader;

/// Retains one native handle and limits consumption without reopening its path.
pub(crate) struct LocalRangeReader {
    /// Native reader with a decreasing byte allowance.
    inner: LimitInput<LocalFileReader>,
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
    ///
    /// # Parameters
    ///
    /// - `reader`: Already-open native regular file handle.
    /// - `offset`: Byte offset from the start of the file.
    /// - `length`: Optional maximum bytes to read; `None` reads through EOF.
    ///
    /// # Returns
    ///
    /// A reader that never consumes more than the resolved window.
    ///
    /// # Errors
    ///
    /// Propagates native seek failures for nonzero offsets inside range.
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
            inner: LimitInput::new(reader, length),
        })
    }
}

impl Input for LocalRangeReader {
    type Item = u8;

    /// Reports whether the wrapped reader can serve bytes from its buffer.
    #[inline]
    fn is_buffered(&self) -> bool {
        self.inner.is_buffered()
    }

    /// Reads bytes from the remaining range into the requested output slice.
    ///
    /// # Safety
    ///
    /// The caller must ensure `index..index + count` is a valid range in
    /// `output`.
    #[inline]
    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        // SAFETY: The caller guarantees the requested output range is valid.
        unsafe { self.inner.read_unchecked(output, index, count) }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Seek;

    use qubit_io::Input;
    use qubit_local_files::LocalFileSystem;

    use super::LocalRangeReader;

    #[test]
    fn test_indexed_window_stops_without_consuming_suffix() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("source");
        std::fs::write(&path, b"012345").expect("seed");
        let file = LocalFileSystem::host()
            .expect("host")
            .open_reader(&path)
            .expect("reader");
        let mut reader = LocalRangeReader::new(file, 1, Some(3)).expect("range");
        let mut bytes = [b'!'; 7];
        // SAFETY: 2..5 is a valid output range.
        let count = unsafe { reader.read_unchecked(&mut bytes, 2, 3) }.expect("read");
        assert_eq!(count, 3);
        assert_eq!(&bytes, b"!!123!!");
        assert_eq!(reader.inner.remaining(), 0);
        assert_eq!(reader.inner.inner_mut().stream_position().expect("position"), 4);
        assert_eq!(reader.read(&mut [0; 1]).expect("window EOF"), 0);
        assert_eq!(reader.inner.inner_mut().stream_position().expect("position"), 4);
    }

    #[test]
    fn test_empty_window_does_not_seek_or_read() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("source");
        std::fs::write(&path, b"012345").expect("seed");
        let file = LocalFileSystem::host()
            .expect("host")
            .open_reader(&path)
            .expect("reader");
        let mut reader = LocalRangeReader::new(file, 5, Some(0)).expect("empty range");
        assert_eq!(reader.inner.inner_mut().stream_position().expect("position"), 0);
        let mut bytes = [b'!'; 2];
        // SAFETY: 2..2 is an empty range at the end of the slice.
        assert_eq!(
            unsafe { reader.read_unchecked(&mut bytes, 2, 0) }.expect("empty read"),
            0
        );
        assert_eq!(reader.read(&mut bytes).expect("window EOF"), 0);
        assert_eq!(&bytes, b"!!");
        assert_eq!(reader.inner.inner_mut().stream_position().expect("position"), 0);
    }
}
