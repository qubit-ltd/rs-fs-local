# qubit-fs-local

[![Rust CI](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-fs-local/coverage-badge.json)](https://qubit-ltd.github.io/rs-fs-local/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-fs-local.svg?color=blue)](https://crates.io/crates/qubit-fs-local)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-fs-local` gives a `qubit-fs` application a synchronous local `file:`
backend. Use it when the application needs the process host filesystem or one
native directory retained as a rooted filesystem authority, without making URI
parsing and native-path conversion part of application code.

## Installation

```bash
cargo add qubit-fs qubit-fs-local
```

Enable registry integration only when registering the optional `file`
provider with `qubit-fs-registry`:

```bash
cargo add qubit-fs-registry
cargo add qubit-fs-local --features registry
```

## Quick Start

For an application that must keep reports below `/srv/app-data`, create a
rooted facade with the application's stable filesystem identity, then use
absolute logical paths inside that authority:

```rust
use std::path::Path;

use qubit_fs::{FileSystemId, Path as LogicalPath};
use qubit_fs_local::LocalFileSystems;

let file_system = LocalFileSystems::rooted_with_id(
    FileSystemId::new("app-data")?,
    Path::new("/srv/app-data"),
)?;
let metadata = file_system.stat(&LogicalPath::parse("/reports/summary.csv")?)?;
println!("{metadata:?}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`LocalFileSystems::host()` opens the process host namespace. `rooted(root)`
generates a process-local identity, while `rooted_with_id(id, root)` preserves
the caller-provided identity. The latter is the appropriate choice when that
identity must be stable outside the process.

## What It Provides

- A concrete synchronous `FileSystem` facade over local files, with host and
  rooted construction paths.
- Optional `LocalFileSystemProvider` registration behind the `registry`
  feature; it resolves supported `file:` configurations to a filesystem, path,
  and canonical URI.
- The provider accepts absolute `file:` URIs only. It rejects a remote
  authority, query, relative path, non-`file` scheme, and configuration options
  or credentials.
- Use `LocalFileSystemProvider::rooted_with_descriptor` when multiple rooted
  local authorities must be registered in one registry; each descriptor ID
  becomes the provider identity exposed by its filesystem.

This crate is synchronous today. It does not provide an asynchronous local
filesystem facade.

## Learn More

- [English user guide](doc/user_guide.md)
- [中文用户手册](doc/user_guide.zh_CN.md)
- [API documentation](https://docs.rs/qubit-fs-local)
- [中文 README](README.zh_CN.md)

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
