//! Thin local-files adapter for [`qubit_fs`].

#![deny(missing_docs)]

mod local_file_systems;
pub mod path;
#[cfg(feature = "registry")]
mod registry;
pub mod spi;

pub use local_file_systems::LocalFileSystems;
#[cfg(feature = "registry")]
pub use registry::LocalFileSystemProvider;
