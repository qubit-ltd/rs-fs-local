# qubit-fs-local

[![Rust CI](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-fs-local/coverage-badge.json)](https://qubit-ltd.github.io/rs-fs-local/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-fs-local.svg?color=blue)](https://crates.io/crates/qubit-fs-local)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-fs-local` provides the synchronous host-local `file:` filesystem backend
for [`qubit-fs`](https://crates.io/crates/qubit-fs). `LocalFileSystems` creates
concrete host-wide or descriptor-rooted facades. Registry integration through
the `file` provider alias is optional behind the `registry` feature.

## Installation

Add the crate to your project. Enable `registry` when the application uses
`LocalFileSystemProvider` with `qubit-fs-registry`:

```bash
cargo add qubit-fs-local
cargo add qubit-fs-local --features registry
```

## Usage

Create a host-wide facade, or open one rooted authority with an explicit stable
identity:

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

Enable `registry`, register `LocalFileSystemProvider`, then resolve a validated
`file:` configuration:

```rust
use qubit_fs::ConnectionUri;
use qubit_fs_registry::FileSystemRegistry;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_registry::FileSystemConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = FileSystemRegistry::default();
    registry.register(LocalFileSystemProvider::new())?;

    let config = FileSystemConfig::new(ConnectionUri::parse("file:///tmp/example.txt")?);
    let resolution = registry.resolve_config(&config)?;
    let metadata = resolution
        .file_system()
        .stat(resolution.path())?;
    println!("{metadata:?}");
    Ok(())
}
```

## Path Semantics

- Both facades use absolute, hierarchical `qubit_fs::Path` values.
- `LocalFileSystems::host()` maps to the process host namespace;
  `rooted_with_id()` retains one native root authority and requires the caller's
  stable `FileSystemId`.
- The adapter delegates canonical component conversion, rooted containment, and
  native filesystem operations to `qubit-local-files`.
- The optional provider accepts only absolute `file:` URIs with no remote
  authority, query, options, metadata, or credentials.

## Properties

Both facades advertise `List`, `Read`, `Write`, `Append`, `CreateDirectory`,
`Delete`, `RecursiveDelete`, `Rename`, `Copy`, temporary resources,
`AtomicReplace`, and `AtomicTempPersist`. Host-dependent limits are reported as
`FileSystemLimits::unknown()`.

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
