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

This README documents `qubit-fs-local` 0.4.

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
use std::time::Duration;

use qubit_fs::Path as LogicalPath;
use qubit_fs::metadata::FileSystemId;
use qubit_fs_local::{
    LocalCopyResourceLimits, LocalDeleteResourceLimits, LocalFileSystems,
    LocalListResourceLimits, LocalResourcePolicy,
};

let policy = LocalResourcePolicy::bounded(
    LocalListResourceLimits::new(32, 10_000, 4 * 1024 * 1024, 32, Duration::from_secs(30))?,
    LocalCopyResourceLimits::new(32, 10_000, 64 * 1024 * 1024, 32, Duration::from_secs(30))?,
    LocalDeleteResourceLimits::new(32, 10_000, 4 * 1024 * 1024, Duration::from_secs(30)),
);
let file_system = LocalFileSystems::rooted_with_id(
    FileSystemId::new("app-data")?,
    Path::new("/srv/app-data"),
    policy,
)?;
let metadata = file_system.stat(&LogicalPath::parse("/reports/summary.csv")?)?;
println!("{metadata:?}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Every constructor requires an explicit `LocalResourcePolicy`. Prefer
`bounded(list, copy, delete)` with all three operation budgets. Use `unbounded()`
only when the application deliberately accepts unbounded recursive work.
`LocalFileSystems::host(policy)` opens the process host namespace. `rooted(root, policy)`
generates a process-local identity, while
`rooted_with_id(id, root, policy)` preserves the caller-provided identity and
applies the same explicit policy. The latter is the appropriate choice when
that identity must be stable outside the process.

When a native host path comes from an API such as `std::env::current_dir`,
convert it with `host_path_to_logical` before passing it to the facade. This
preserves percent escapes and non-UTF-8 Unix names. Use `qubit_fs::Path::parse`
for a path that is already a rooted logical path; the two representations have
different authorities.

```rust
use std::time::Duration;
use qubit_fs::read::ReadOptions;
use qubit_fs_local::{
    host_path_to_logical, LocalCopyResourceLimits, LocalDeleteResourceLimits,
    LocalFileSystems, LocalListResourceLimits, LocalResourcePolicy,
};

let policy = LocalResourcePolicy::bounded(
    LocalListResourceLimits::new(16, 1_000, 1 << 20, 16, Duration::from_secs(10))?,
    LocalCopyResourceLimits::new(16, 1_000, 16 << 20, 16, Duration::from_secs(10))?,
    LocalDeleteResourceLimits::new(16, 1_000, 1 << 20, Duration::from_secs(10)),
);
let file_system = LocalFileSystems::host(policy)?;
let native = std::env::current_dir()?.join("Cargo.toml");
let path = host_path_to_logical(&native)?;
let prefix = file_system.read_prefix(&path, ReadOptions::default(), 4096)?;
assert!(prefix.len() <= 4096);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`bounded(list, copy, delete)` sets explicit per-request ceilings
for the three ordinary operation categories.
The three limit families are independent: listing and copy use their own
depth/entry/byte/open-directory/deadline values, while deletion uses depth,
entry, pending-path-byte, and deadline values. A provider listing entry ceiling counts
entries yielded by the native walker before prefix filtering, while a request
entry limit counts entries returned after filtering. Temporary-resource
cleanup, Drop, and native publication cleanup do not inherit ordinary deletion
ceilings. These settings are not aggregate quotas across concurrent requests.

## What It Provides

- A concrete synchronous `FileSystem` facade over local files, with host and
  rooted construction paths.
- Optional `LocalFileSystemProvider` registration behind the `registry`
  feature; it resolves supported `file:` configurations to a filesystem, path,
  and canonical URI.
- The provider accepts absolute `file:` URIs only. It rejects a remote
  authority, query, relative path, non-`file` scheme, and configuration options
  or credentials.
- Rooted provider construction opens the native authority immediately. If it
  fails, construction returns an error and no provider is available for
  registration or fallback. The default `rooted` descriptor is `local-file`
  with the `file` alias; `rooted_with_descriptor` preserves the supplied
  descriptor and aliases unchanged.
- Use `LocalFileSystemProvider::rooted_with_descriptor` when multiple rooted
  local authorities must be registered in one registry; each descriptor ID
  becomes the provider identity exposed by its filesystem.

Fallback is enabled only by an explicit provider `chain`, an automatic
`ProviderSelection::auto()` selection, or the registry's default selection.
When `FileSystemConfig` has no selection, `resolve_config` derives a named
selection from the URI scheme; that single-provider selection never falls back.
With `FallbackPolicy::OnAbsence`, a `file:` configuration error remains
terminal, while an unsupported scheme may continue to another provider.

This crate is synchronous today. It does not provide an asynchronous local
filesystem facade.

## Learn More

- [English user guide](doc/user_guide.md)
- [中文用户手册](doc/user_guide.zh_CN.md)
- [Adapter design](doc/local_file_system_adapter_design.md)
- [中文设计文档](doc/local_file_system_adapter_design.zh_CN.md)
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
