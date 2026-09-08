# qubit-fs-local User Guide

[中文](user_guide.zh_CN.md) · [README](../README.md) · [API documentation](https://docs.rs/qubit-fs-local)

## Purpose and Audience

This guide is for Rust applications using `qubit-fs` that need a synchronous
filesystem backed by the local host. It covers the current `qubit-fs-local`
0.5.0 release: direct host/rooted facades and the optional registry provider
(package version `0.5.0`).

## Provider resource ceilings

Native `LocalFileSystem` defaults are replaceable convenience Options. This
adapter treats `LocalResourcePolicy` as per-request ceilings: list/copy request
limits are intersected with provider limits, and omitting a request limit cannot
remove a provider limit. These are not aggregate quotas across concurrent requests.

`LocalResourcePolicy::bounded(list, copy, delete)` sets independent ceilings for
the three ordinary recursive operation families. Deletion requests preserve
these ceilings while selecting recursion and missing-entry behavior. The requested
directory counts as one entry at depth zero; pending-path bytes measure encoded
native lengths, excluding allocator overhead. Deadlines are cooperative.
`with_delete_limits(None)` explicitly removes deletion ceilings at provider
construction time. No deletion limit is inferred from a listing or copy limit.

The three limit families count different work:

| Limit | Counted work | Applied by |
| --- | --- | --- |
| Provider list `max_entries` | Entries yielded before adapter prefix filtering | Native walker |
| Request list `max_entries` | Entries returned after prefix filtering | Facade |
| Copy/delete limits | Native traversal and payload work for that operation | Native operation |

Consequently, a prefix with no matches can still exhaust the provider walker
ceiling. A cooperative deadline is checked around native calls and cannot
interrupt an I/O call already in progress.

The following values are application scenario examples, not library defaults:

```rust
use std::time::Duration;
use qubit_fs_local::{
    LocalCopyResourceLimits, LocalDeleteResourceLimits, LocalFileSystems,
    LocalListResourceLimits, LocalResourcePolicy,
};

let policy = LocalResourcePolicy::bounded(
    LocalListResourceLimits::new(
        32, 10_000, 4 * 1024 * 1024, 32, Duration::from_secs(30),
    )?,
    LocalCopyResourceLimits::new(
        32, 10_000, 64 * 1024 * 1024, 32, Duration::from_secs(30),
    )?,
    LocalDeleteResourceLimits::new(
        32, 10_000, 4 * 1024 * 1024, Duration::from_secs(30),
    ),
);
let file_system = LocalFileSystems::host(policy)?;
# let _ = file_system;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Logical path text limits are reported as `Unknown` because native component
limits do not account for percent expansion or non-UTF-8 names. Convert an
absolute native host path with `host_path_to_logical`; parse a path that is
already rooted logical text with `qubit_fs::Path::parse`. Temporary-resource
cleanup, Drop, and native publication cleanup have an independent lifecycle
scope and do not inherit ordinary deletion ceilings. Explicit cleanup reports
its own error; Drop is best effort. The local provider takes over every copy it
can express and does not decline into a facade fallback. Copy failures after
native work starts retain their state and partial statistics.

The adapter preserves native deletion classification: deleting a directory through
`delete_file` reports `IsDirectory`, and deleting a regular file or final symbolic
link through `delete_directory` reports `NotDirectory` without removing it. If
recursive deletion has already removed entries, inspect the returned `FsError`
effect state before retrying. Its native `LocalFileError` source exposes
`cause_kind()` separately; an absent effect state means that no effect was proven.

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
let fs = LocalFileSystems::rooted_with_id(
    FileSystemId::new("app-data")?,
    Path::new("/srv/app-data"),
    policy,
)?;
let report = LogicalPath::parse("/reports/summary.csv")?;
let metadata = fs.stat(&report)?;
# let _ = metadata;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Every constructor requires `LocalResourcePolicy`; pass complete bounded list,
copy, and delete limits for ordinary recursive workflows. Use `unbounded()` only
after explicitly accepting unbounded recursive resource use. Choose `host(policy)` only when the intended authority is the process host
namespace. Use `rooted_with_id` when the filesystem identity must be supplied
by the application; use `rooted` when a distinct process-local identity is
sufficient.

## Advanced Usage

Register a local provider to resolve a validated `file:` URI at application
assembly time:

```rust
use qubit_fs::path::ConnectionUri;
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
If opening the authority fails, construction returns an error, so no provider is
registered or considered by a later fallback. The convenience `rooted`
constructor uses the default `local-file` descriptor with the `file` alias.
Use `rooted_with_descriptor` when registering multiple rooted authorities in one
registry; each descriptor ID must be distinct and becomes the filesystem
provider identity. The supplied descriptor is preserved exactly, including its
aliases; the constructor does not add the default `file` alias.

For example, two rooted providers can resolve the same logical path while
retaining separate native authorities and identities:

```rust
use std::path::Path as NativePath;

use qubit_fs::{metadata::FileSystemId, path::ConnectionUri};
use qubit_fs_local::{LocalFileSystemProvider, LocalResourcePolicy};
use qubit_fs_registry::{FileSystemConfig, FileSystemRegistry};
use qubit_spi::{ProviderDescriptor, ProviderId, ProviderSelection};

let registry = FileSystemRegistry::default();
registry.register(LocalFileSystemProvider::rooted_with_descriptor(
    ProviderDescriptor::new(ProviderId::new("tenant-a")?),
    FileSystemId::new("tenant-a-files")?,
    NativePath::new("/srv/tenant-a"),
    LocalResourcePolicy::unbounded(),
)?)?;
registry.register(LocalFileSystemProvider::rooted_with_descriptor(
    ProviderDescriptor::new(ProviderId::new("tenant-b")?),
    FileSystemId::new("tenant-b-files")?,
    NativePath::new("/srv/tenant-b"),
    LocalResourcePolicy::unbounded(),
)?)?;

let uri = ConnectionUri::parse("file:///reports/summary.csv")?;
let first = registry.resolve_config(&FileSystemConfig::new(uri.clone()).with_selection(
    ProviderSelection::named("tenant-a")?,
))?;
let second = registry.resolve_config(&FileSystemConfig::new(uri).with_selection(
    ProviderSelection::named("tenant-b")?,
))?;
assert_eq!(first.canonical_uri(), second.canonical_uri());
assert_ne!(
    first.file_system().properties().info().id(),
    second.file_system().properties().info().id(),
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The canonical URI for both resolutions is `file:///reports/summary.csv`: it
contains the provider-decoded logical path and carries no rooted native
authority. A canonical URI therefore cannot recover a rooted provider or a
filesystem identity by itself. Persist a provider selection/descriptor and
filesystem identity with the URI. Replaying only the canonical URI through a
host provider produces the same URI and logical path, but resolves to the
`local-host` filesystem rather than either rooted filesystem.

## Errors and Diagnostics

Native operation failures are reported through the `qubit-fs` error model; the
local adapter identifies itself as `local-file`. Registry creation and
resolution errors are returned by `qubit-fs-registry`. Inspect the returned
error rather than assuming a URI was accepted.

The provider rejects configurations outside its local-file contract: a remote
authority, query, relative path, non-`file` scheme, options, and credentials.
It decodes percent-encoded path bytes per URI component; encoded separators
and NUL bytes are rejected so they cannot alter the logical hierarchy.

For a non-`file` scheme, the provider returns `Unsupported` with an
`UnsupportedOperation` error. A registry using `FallbackPolicy::OnAbsence` may
continue to another provider. Once the scheme is `file`, configuration errors
such as a remote authority, query, options, credentials, or an invalid path
remain terminal; `OnAbsence` does not skip them. Failure to open a rooted
authority is also terminal initialization failure.

Fallback requires an explicit `ProviderSelection::chain`, an automatic
`ProviderSelection::auto()`, or resolution through the registry's default
selection. When `FileSystemConfig` has no selection,
`FileSystemRegistry::resolve_config` derives a named selection from the URI
scheme; named selection targets one provider and never falls back.

For native `PublicationIncomplete`, the adapter reports
`FsEffectState::PartiallyApplied` and keeps the underlying native error as the
source. Cause and effect are independent: the effect state describes namespace
progress, while the mapped error kind follows the native cause when available.

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
- Keep the provider selection and filesystem identity alongside a canonical URI
  when a rooted resolution must be replayed; the URI alone does not identify
  its retained native authority.

## Further Reading

- [README](../README.md)
- [中文用户手册](user_guide.zh_CN.md)
- [API documentation](https://docs.rs/qubit-fs-local)
