//! Lazy directory walker adapter.

use qubit_fs::spi::DirectoryStreamSpi;
use qubit_fs::{DirEntry, FsError, FsErrorKind, FsOperation, FsResult, Path};
use qubit_local_files as native_files;

use crate::path::LocalPathMapper;

pub(crate) enum LocalDirectoryStreamSpi {
    Host(native_files::LocalDirectoryWalker),
    Rooted(native_files::LocalDirectoryWalker, Path),
}
impl LocalDirectoryStreamSpi {
    pub(crate) const fn host(walker: native_files::LocalDirectoryWalker) -> Self {
        Self::Host(walker)
    }
    pub(crate) fn rooted(walker: native_files::LocalDirectoryWalker, root: Path) -> Self {
        Self::Rooted(walker, root)
    }
}
impl DirectoryStreamSpi for LocalDirectoryStreamSpi {
    fn next_entry(&mut self) -> FsResult<Option<DirEntry>> {
        let (entry, rooted) = match self {
            Self::Host(walker) => (walker.next(), None),
            Self::Rooted(walker, root) => (walker.next(), Some(root.clone())),
        };
        let Some(entry) = entry else {
            return Ok(None);
        };
        let entry = entry.map_err(|error| {
            FsError::with_source(
                FsErrorKind::Io,
                FsOperation::List,
                "native directory walk failed",
                error,
            )
        })?;
        let path = if let Some(root) = rooted {
            let relative = LocalPathMapper::rooted_logical(entry.relative_path())?;
            if relative == Path::root() {
                root
            } else if root == Path::root() {
                relative
            } else {
                Path::parse(&format!("{}/{}", root.as_str(), &relative.as_str()[1..]))?
            }
        } else {
            LocalPathMapper::host_logical(entry.path())?
        };
        Ok(Some(DirEntry::new(
            path,
            match entry.metadata().kind() {
                native_files::LocalFileKind::File => qubit_fs::FileKind::File,
                native_files::LocalFileKind::Directory => qubit_fs::FileKind::Directory,
                native_files::LocalFileKind::Symlink => qubit_fs::FileKind::Symlink,
                native_files::LocalFileKind::Other => qubit_fs::FileKind::Other("local".to_owned()),
            },
        )))
    }
}
