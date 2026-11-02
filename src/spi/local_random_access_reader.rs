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
        Self {
            reader,
            path,
            provider_id,
        }
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

#[cfg(test)]
mod tests {
    use std::error::Error;

    use qubit_fs::error::FsErrorKind;
    use qubit_fs::error::FsOperation;
    use qubit_fs::path::Path;
    use qubit_fs::spi::RandomAccessReadSession;
    use qubit_local_files as native_files;

    use super::LocalRandomAccessReader;

    #[test]
    fn test_empty_read_does_not_seek() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let native_path = directory.path().join("payload");
        std::fs::write(&native_path, b"abcdef").expect("seed file");
        let logical_path = Path::parse("/payload").expect("logical path");
        let native_fs = native_files::LocalFileSystem::host().expect("host filesystem");
        let reader = native_fs.open_reader(&native_path).expect("open native file");
        let mut session = LocalRandomAccessReader::new(reader, logical_path.clone(), "local-test".to_owned());

        assert_eq!(0, session.read_at(u64::MAX, &mut []).expect("empty read"));
    }

    #[cfg(unix)]
    #[test]
    fn test_failed_seek_preserves_context_and_can_be_retried() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let native_path = directory.path().join("payload");
        std::fs::write(&native_path, b"abcdef").expect("seed file");
        let logical_path = Path::parse("/payload").expect("logical path");
        let native_fs = native_files::LocalFileSystem::host().expect("host filesystem");
        let reader = native_fs.open_reader(&native_path).expect("open native file");
        let mut session = LocalRandomAccessReader::new(reader, logical_path.clone(), "local-test".to_owned());

        let error = session
            .read_at(u64::MAX, &mut [0; 1])
            .expect_err("an unrepresentable seek must fail");
        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::Read, error.operation());
        assert_eq!(Some(&logical_path), error.path());
        assert_eq!(Some("local-test"), error.provider());
        assert!(error.source().is_some());

        let mut output = [0; 2];
        assert_eq!(2, session.read_at(0, &mut output).expect("read after failed seek"));
        assert_eq!(*b"ab", output);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_native_read_error_preserves_context_and_source() {
        let logical_path = Path::parse("/proc/self/mem").expect("logical path");
        let native_fs = native_files::LocalFileSystem::host().expect("host filesystem");
        let reader = native_fs
            .open_reader(std::path::Path::new("/proc/self/mem"))
            .expect("open process memory file");
        let mut session = LocalRandomAccessReader::new(reader, logical_path.clone(), "local-test".to_owned());

        let error = session
            .read_at(0, &mut [0; 1])
            .expect_err("unmapped process memory must fail to read");
        assert_eq!(FsErrorKind::Io, error.kind());
        assert_eq!(FsOperation::Read, error.operation());
        assert_eq!(Some(&logical_path), error.path());
        assert_eq!(Some("local-test"), error.provider());
        assert!(error.source().is_some());
    }
}
