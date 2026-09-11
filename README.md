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

This README documents `qubit-fs-local` 0.8.0 (package version `0.8.0`).

This version uses `qubit-fs` 0.7 and `qubit-local-files` 0.3. Provider ceilings tighten resources
without overriding request behavior; writers preserve existing metadata.
Logical path and temporary publication contracts remain those of the portable
facade. See the [user guide](doc/user_guide.md#provider-resource-ceilings).

## Installation

```bash
cargo add qubit-fs@0.7 qubit-fs-local@0.8
```

Enable registry integration only when registering the optional `file`
provider with `qubit-fs-registry`:

```bash
cargo add qubit-fs-registry@0.6
cargo add qubit-fs-local@0.8 --features registry
```

## Quick Start

For an application that must keep reports below `/srv/app-data`, create a
rooted facade with the application's stable filesystem identity, then use
absolute logical paths inside that authority:

```rust
use std::path::Path;

use qubit_fs::Path as LogicalPath;
use qubit_fs::metadata::FileSystemId;
use qubit_fs_local::{LocalFileSystems, LocalResourcePolicy};

let policy = LocalResourcePolicy::standard();
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
`standard()` for finite budgets, or customize `bounded(list, copy, delete)`. Use `unbounded()`
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
use qubit_fs::read::ReadOptions;
use qubit_fs_local::{host_path_to_logical, LocalFileSystems, LocalResourcePolicy};

let policy = LocalResourcePolicy::standard();
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

Temporary resources preserve the requested logical parent spelling, including
directory aliases, in their returned paths and generated keep targets. Explicit
publication reports the requested logical target. Native guards retain their
original creation authority and cleanup paths throughout these operations.
Persistence failures keep the current target-publication fact separate from
source qualification. An invalid retry, cleanup error, or cancellation cannot
erase an earlier confirmed `publication_target`; source-indeterminate states
permit read-only reconciliation, while cleanup-required states permit cleanup
without another publication attempt.

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

## Filesystem contract update

Listing requires `ListScope::Path(path)`; use `ListScope::Path(Path::root())`
for the configured hierarchical root. `ListScope::Namespace` is rejected before
native I/O. This provider advertises Conditional `RangeRead` for native regular-file windows.
The adapter seeks and reads through the same opened native handle, retaining full
resource metadata. Zero-length, EOF and beyond-EOF windows are empty after checking
resource existence; missing paths still fail. This does not promise a snapshot
under concurrent mutation. Automatic `read_prefix` narrowing requires Guaranteed
range support, so local prefix reads continue to use bounded sequential consumption.

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
