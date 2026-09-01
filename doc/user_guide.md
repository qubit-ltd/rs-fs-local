# qubit-fs-local User Guide

[中文](user_guide.zh_CN.md) · [README](../README.md) · [API documentation](https://docs.rs/qubit-fs-local)

## Purpose and Audience

This guide is for Rust applications using `qubit-fs` that need a synchronous
filesystem backed by the local host. It covers the current `qubit-fs-local`
0.1 release: direct host/rooted facades and the optional registry provider.

## Conceptual Model

`LocalFileSystems` is a factory for a concrete `FileSystem` facade.

```text
application logical Path
        │
        ├─ host() ──────────────► process host namespace
        └─ rooted*_with_id() ───► one retained native root
```

The facade accepts absolute, hierarchical `qubit_fs::Path` values. A rooted
facade keeps a native root and uses logical paths below it. `rooted` creates a
process-local ID; `rooted_with_id` uses the supplied `FileSystemId`.

With the `registry` feature, `LocalFileSystemProvider` is a provider factory
for supported `file:` configurations. A successful registry resolution exposes
the configured filesystem, provider-decoded logical path, and a canonical URI.

## Scenario

An application stores generated reports beneath `/srv/app-data` and must not
treat that native root as an application path on every operation. The success
condition is that `/reports/summary.csv` is addressed as a logical path through
one rooted facade.

## Installation and Minimal Configuration

```bash
cargo add qubit-fs qubit-fs-local
```

For registry use, enable the feature and add the registry crate in the
application:

```bash
cargo add qubit-fs-registry
cargo add qubit-fs-local --features registry
```

## Core Workflow

```rust
use std::path::Path;

use qubit_fs::{FileSystemId, Path as LogicalPath};
use qubit_fs_local::{LocalFileSystems, LocalResourcePolicy};

let fs = LocalFileSystems::rooted_with_id(
    FileSystemId::new("app-data")?,
    Path::new("/srv/app-data"),
    LocalResourcePolicy::unbounded(),
)?;
let report = LogicalPath::parse("/reports/summary.csv")?;
let metadata = fs.stat(&report)?;
# let _ = metadata;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Every constructor requires `LocalResourcePolicy`; use `unbounded()` only after
explicitly accepting unbounded recursive resource use, or pass complete bounded
list and copy limits. Choose `host(policy)` only when the intended authority is the process host
namespace. Use `rooted_with_id` when the filesystem identity must be supplied
by the application; use `rooted` when a distinct process-local identity is
sufficient.

## Advanced Usage

Register a local provider to resolve a validated `file:` URI at application
assembly time:

```rust
use qubit_fs::ConnectionUri;
use qubit_fs_local::{LocalFileSystemProvider, LocalResourcePolicy};
use qubit_fs_registry::{FileSystemConfig, FileSystemRegistry};

let registry = FileSystemRegistry::default();
registry.register(LocalFileSystemProvider::host(LocalResourcePolicy::unbounded()))?;
let config = FileSystemConfig::new(ConnectionUri::parse("file:///tmp/report.csv")?);
let resolution = registry.resolve_config(&config)?;
let _metadata = resolution.file_system().stat(resolution.path())?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`LocalFileSystemProvider::rooted(id, root, policy)` opens the supplied native authority
during provider construction and returns `FsResult<LocalFileSystemProvider>`.
Every later resolution reuses that opened authority instead of reopening the
configured root path.
Use `rooted_with_descriptor` when registering multiple rooted authorities in one
registry; each descriptor ID must be distinct and becomes the filesystem
provider identity.

## Errors and Diagnostics

Native operation failures are reported through the `qubit-fs` error model; the
local adapter identifies itself as `local-file`. Registry creation and
resolution errors are returned by `qubit-fs-registry`. Inspect the returned
error rather than assuming a URI was accepted.

The provider rejects configurations outside its local-file contract: a remote
authority, query, relative path, non-`file` scheme, options, and credentials.
It decodes percent-encoded path bytes per URI component; encoded separators
and NUL bytes are rejected so they cannot alter the logical hierarchy.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| `LocalFileSystemProvider` is unavailable | Enable the `registry` feature. |
| A `file:` URI does not resolve | Verify it is absolute, has no remote authority or query, and has no options or credentials. |
| A rooted facade cannot be opened | Verify the native root exists and can be opened by the process. |
| A path cannot be used | Use an absolute hierarchical `qubit_fs::Path` within the selected facade's authority. |

## Limitations and Best Practices

- The public facade is synchronous; this crate has no asynchronous local
  filesystem facade.
- Rooted containment and native filesystem behavior are provider boundaries;
  retain the rooted facade instead of repeatedly joining native paths in the
  application.
- Treat a `file:` URI as local-only input. Remote authorities and URI options
  are intentionally not configuration channels for this provider.

## Further Reading

- [README](../README.md)
- [中文用户手册](user_guide.zh_CN.md)
- [API documentation](https://docs.rs/qubit-fs-local)
