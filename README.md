# qubit-fs-local

[![Rust CI](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-fs-local/coverage-badge.json)](https://qubit-ltd.github.io/rs-fs-local/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-fs-local.svg?color=blue)](https://crates.io/crates/qubit-fs-local)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-fs-local` provides the synchronous host-local `file:` filesystem backend
for [`qubit-fs`](https://crates.io/crates/qubit-fs). It exposes mandatory
metadata lookup, synchronous reads and writes, and
`RootedLocalFileSystem` for descriptor-relative authority. Registry integration
through the `file` provider alias is optional behind the `registry` feature.

## Installation

Add the crate to your project:

```bash
cargo add qubit-fs-local
```

## Usage

Enable `registry`, register `LocalFileSystemProvider`, then resolve a local resource from a
validated `file:` URI:

```rust
use qubit_fs::{FsUri, ReadOptions};
use qubit_fs_registry::FileSystemRegistry;
use qubit_fs_local::LocalFileSystemProvider;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = FileSystemRegistry::default();
    registry.register(LocalFileSystemProvider)?;

    let uri = FsUri::parse("file:///tmp/example.txt")?;
    let resource = registry.resource_uri(&uri)?;
    let metadata = resource.stat()?;
    let content = resource.read_all(1024 * 1024)?;
    let _streaming_reader = resource.open_reader(ReadOptions::default())?;
    println!("{metadata:?}");
    println!("read {} bytes", content.len());
    Ok(())
}
```

## Path Semantics

- `LocalFileSystem` has host-wide authority and accepts native absolute paths
  only.
- Canonical `FsPath` text is converted component by component through
  `OsStrPathCodec`, preserving literal percent characters, Unix non-UTF-8
  bytes, and Windows native path code units without letting decoded separators
  cross a component boundary.
- `stat` uses `symlink_metadata`, so it reports the final symbolic link itself
  instead of following it.
- `open_reader` performs blocking local I/O and accepts `ReadOptions` by value.
  The current backend supports sequential whole-file reads only; range,
  conditional, and required-checksum requests fail during preflight.
- The provider accepts `file:` URIs with no authority or an empty authority. It
  rejects remote authorities, queries, provider options, and credentials.

## Properties

`LocalFileSystem` advertises `FileSystemCapability::Read`. `stat` is part of the
base filesystem contract and therefore has no capability flag. Host-dependent
path and I/O limits are reported explicitly as `FileSystemLimits::unknown()`.

This release is synchronous only. If asynchronous support is added later, it
will be published as an opt-in feature rather than increasing the default
dependency surface.

## Testing

```bash
# Run tests with the default feature set
cargo test

# Run tests with all declared features
cargo test --all-features

# Project CI checks
./ci-check.sh

# Check code coverage
./coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./align-ci.sh` to format code and
`./ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-fs-local](https://github.com/qubit-ltd/rs-fs-local)
