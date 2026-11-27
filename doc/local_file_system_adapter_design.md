# Qubit FS Local Adapter Design

> Approved target design for `qubit-fs-local` 0.12.0 (package version `0.12.0`), reviewed against the public
> `qubit-fs` 0.4 and `qubit-local-files` 0.6.0 boundaries. Implementation and regression
> tests converge on the contracts below.
>
> [中文设计文档](local_file_system_adapter_design.zh_CN.md) · [User guide](user_guide.md)

## 1. Positioning

`qubit-fs-local` is a thin adapter between `qubit-fs` and `qubit-local-files`:

```text
qubit-fs FileSystem facade
          │ FileSystemSpi
          ▼
LocalFileSystemSpi ──► LocalPathMapper / path projection
       │              ├─ LocalRandomAccessReader
       │              └─ LocalRangeReader
       └─► LocalTempResourceSpi ──► LocalTempCleanupControl
          │ native operations and cleanup lifecycle
          ▼
qubit-local-files native semantics and platform implementation
```

It translates types, paths, errors, outcomes, and sessions; it does not
implement a second local filesystem algorithm.

## 2. Goals and non-goals

Goals are a synchronous local `FileSystem` facade, the
`qubit_fs::spi::FileSystemSpi` implementation, delegation of native business
logic to `qubit-local-files`, accurate configured-filesystem properties,
lossless provider-neutral outcomes and partial-success states, optional
`file:` registry integration, and no operation SPI in the ordinary application
API. It does not implement copy, walking, temporary resources, publication,
durability, root containment, native platform algorithms, generic request
revalidation, an async runtime wrapper, or an async facade.

## 3. Dependencies and naming

```rust
use qubit_local_files as native_files;
```

The layers are `qubit_fs::FileSystem`,
`qubit_fs_local::spi::LocalFileSystemSpi`, and
`qubit_local_files::LocalFileSystem`. Host and rooted modes share one
`LocalFileSystemSpi`; its configured native instance determines the namespace.

## 4. Public construction

`LocalFileSystems` is an empty enum used only as a type namespace:

```rust
pub enum LocalFileSystems {}

impl LocalFileSystems {
    pub fn host(policy: LocalResourcePolicy) -> FsResult<FileSystem>;
    pub fn rooted(root: &Path, policy: LocalResourcePolicy) -> FsResult<FileSystem>;
    pub fn rooted_with_id(
        id: FileSystemId, root: &Path, policy: LocalResourcePolicy,
    ) -> FsResult<FileSystem>;
}
```

`rooted` creates a process-local identity; `rooted_with_id` accepts an identity
for registry, persistence, and tests. Identity is not derived from sensitive
native path text. Every constructor requires an explicit policy.

<!-- fs-local-example: direct -->
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
use qubit_fs::Path as LogicalPath;
use qubit_fs_local::{LocalFileSystems, LocalResourcePolicy};
use std::path::Path as NativePath;

let fs = LocalFileSystems::rooted(
    NativePath::new("/data"), LocalResourcePolicy::standard(),
)?;
let metadata = fs.stat(&LogicalPath::parse("/reports/summary.csv")?)?;
let _ = metadata;
    Ok(())
}
```

`LocalResourcePolicy::bounded(list, copy, delete)` sets independent per-request
ceilings for listing, copying, and ordinary recursive deletion;
`unbounded()` is an explicit opt-in. Listing counts native walker work before
prefix filtering; request limits count filtered entries. Delete counts the
requested root at depth zero and pending encoded native path bytes. Deadlines
are cooperative. Temporary cleanup, `Drop`, and native publication cleanup do
not inherit ordinary deletion ceilings, and these settings are not aggregate
concurrent quotas.

The native temporary-directory API supports independent cleanup limits and
replacing them between attempts. The adapter exposes that capability through
an optional shared control on new entry points; existing constructors retain
their unbounded cleanup behavior. Listing depth is a normal traversal boundary:
EOF at the selected depth does not establish completeness of deeper levels.

Use `standard()` for ordinary workflows; choose `unbounded()` only when accepting unlimited recursive work.

## 5. SPI implementation

Construction creates the native filesystem/context, applies the policy, takes a
properties snapshot, and calls `FileSystem::from_spi`. It returns the concrete
facade, not `Arc<dyn FileSystemSpi>`. The public facade does not expose
operation SPI types.

## 6. Properties

`properties()` returns immutable, I/O-free `ProviderProperties` containing
`FileSystemInfo` (configured `FileSystemId`, provider ID, and
`PathSemantics::Hierarchical`), host/rooted authority description, native
`FileSystemCapabilities`, conservative `FileSystemLimits`, host-absolute or
rooted-either-form `PathConstraints`, and safe diagnostics. Capabilities such as atomic
rename/replace, atomic temporary persist, and durability are conditional on
native evidence. `FileSystemLimits::unknown()` is retained where native units
cannot represent logical UTF-8 text limits because of percent expansion and
non-UTF-8 names. Properties do not change after an operation.

## 7. Path mapping

### 7.1 Codec ownership

The private `path::local_path_mapper` delegates component codec and
`LocalPaths` handling to `qubit-local-files`:

```text
qubit_fs::Path canonical components
  → path::local_path_mapper::{native,logical}
  → native_files::path::LocalPaths / LocalPathCodec
  → native path
```

Raw bytes, Windows UTF-16/WTF-8, escapes, separators, roots, and prefixes are
native responsibilities. `host_path_to_logical` accepts an absolute host path
and preserves non-UTF-8 names and percent escapes without lossy display text.

### 7.2 Host filesystem

Host logical paths are absolute and hierarchical. The mapper uses
`LocalPaths::from_canonical_components` in `Host` scope and never depends on
the current working directory. Unix `/` is native root; the initial Windows
form is `/<drive>:/...`, not a cross-drive global root.

### 7.3 Rooted filesystem

Rooted logical `/` denotes the retained native authority. Absolute paths start
there; relative paths resolve from the native virtual working directory, which
starts at `/`. Components are mapped in `Rooted` scope; containment, descriptors,
symlinks, and race handling remain in the native service.

### 7.4 Multiple and returned paths

Copy, rename, and persist convert every path before metadata or native I/O, so a
later conversion failure cannot leave an earlier side effect. Returned native
paths are encoded with `LocalPaths::to_canonical_components` in the matching
scope. Diagnostic paths never replace provider identity or enter canonical URIs.

## 8. Operation mapping

| `qubit_fs::spi` | `qubit-local-files` |
| --- | --- |
| `StatRequest` | `metadata` |
| `ListRequest` | `LocalDirectoryWalker` / directory stream |
| `OpenReaderRequest` | `open_reader` |
| `OpenReaderRequest (open_random_access_reader)` | `LocalFileReader` + `LocalRandomAccessReader` |
| `OpenWriterRequest` | `LocalFileWriter` |
| `CreateDirectoryRequest` | `create_directory` |
| `DeleteFileRequest` | `delete_file` |
| `DeleteDirectoryRequest` | `delete_directory` |
| `CopyRequest` | `try_copy` / native `copy` |
| `RenameRequest` | `rename` |
| `CreateTempFileRequest` | `LocalTempFile` |
| `CreateTempDirectoryRequest` | `LocalTempDirectory` |

Native deletion classification remains `IsDirectory` for `delete_file` on a
directory and `NotDirectory` for `delete_directory` on a regular file/final
symlink; the latter is not removed. `PublicationIncomplete` maps to
`FsEffectState::PartiallyApplied`; missing evidence is not `Unchanged`. Generic
options are not revalidated; unsupported fields return a contract error.

### 8.1 Random-access reader

`open_random_access_reader` resolves the logical path, maps read options
(including the open retry timeout), opens one native reader, and retains that
handle in `LocalRandomAccessReader`. Each `read_at` uses the retained handle;
it does not reopen the path. The returned length and metadata describe the
observation made at open time. The `qubit-fs` facade clips requests to that
length, so later growth does not extend this session's boundary; truncation can
produce a short read. Replacing the path does not switch an open session to a
new object, but this does not promise consistency against concurrent writes.

The facade checks range overflow and provider resource limits before calling
the session; an empty buffer or a request clipped to zero can return without an
SPI call. Overflow is a facade `InvalidOptions` error. For a non-empty request,
the SPI seeks with `SeekFrom::Start` and reads from the retained handle; seek or
read errors are mapped by `error_mapper` with `Read`, the logical path, provider
identity, and source. Open failures carry `OpenReader` context. These native
seek failures must not be described as the facade's range-validation error.

See the [random-access regression tests](../tests/local_random_access_reader_tests.rs)
and [SPI session implementation](../src/spi/local_random_access_reader.rs).
The tests distinguish shared session semantics from platform-specific path
replacement and seek-error cases; Linux/Unix and Windows coverage is guarded
separately in the test source, and this document makes no claim that an
unexecuted platform test passed.

### 8.2 Copy fast path and states

`try_copy` converts both paths and classifies options before side effects. An
unrepresentable request is `RequirementNotMet`/`Unchanged` with zero partial
statistics. Once native copy starts, every native failure is terminal and never
becomes facade fallback.

```text
not entered native copy → SpiCopyFailure(RequirementNotMet, Unchanged, zero stats)
native copy succeeds     → CopyAttempt::Completed(mapped outcome)
native copy fails        → SpiCopyFailure(mapped state + partial stats)
```

`LocalCopyFailureState::{Unchanged, PartiallyPublished, Published,
Indeterminate}` maps one-to-one with native resource-limit statistics; staging
cleanup errors remain typed diagnostics. A completed `Skip` result is marked
unchanged only when it records at least one skipped item and every copy,
publication, byte, overwrite, and failure count is zero. That no-publication
outcome reports `NonAtomic` and non-durable; the facade may accept it even when
the request required atomicity or durability because no copy operation was
performed. Mixed or partially completed copies do not use this exception.

### 8.3 Rename primitive and states

Rename always calls native rename, never copy-plus-delete. Its
`LocalRenameFailureState::{Unchanged, Renamed, Indeterminate}` maps to
`SpiRenameFailure`; a rename completed before parent durability failure remains
`Renamed`.

## 9. Readers, writers, and streams

### 9.1 Reader

`OpenedReader` uses the properties' `FileSystemId` and requested logical path,
and includes a snapshot only when native open supplied one; it does not issue an
extra `stat`.

### 9.2 Writer

```text
native_files::LocalFileWriter → local writer adapter
                              → qubit_fs::spi::FileWriterSpi
```

I/O errors, publication method, achieved atomicity/durability, and
`RetryableNotPublished`, `NotPublished`, `Published`, and `Indeterminate` are
preserved. The adapter does not rename, fsync, or clean staging itself.

Writer options select native `LocalWriteMetadataPolicy::PreserveExisting`.
This affects staged replacement of an existing destination: Unix preserves
owner, permissions, and supported extended metadata; Windows Host uses native
replacement merging, and Windows Rooted preserves portable permissions. The
policy does not govern first creation, which publishes staging metadata under
native inheritance rules, or append, which follows native write behavior.

### 9.3 Directory stream

The lazy native walker is adapted entry by entry to `DirEntry`; it is not
preloaded into `Vec`, and native continuation handles are not exposed. For a
`Subtree` listing filter, `LocalFileSystemSpi` converts the canonical logical
prefix through the rooted path codec and configures native descent pruning;
the stream still applies the result filter and path projection. The original
listing root remains in force for depth and budgets. The native entry budget
counts observed entries, including ancestors and siblings already encountered,
while pruned descendants consume no entries. Errors below pruned directories
are not observed; errors on entries already visited can still fail the stream.

## 10. Temporary resources

```text
LocalTempFile / LocalTempDirectory → temp session adapter
                                  → OpenedTempFile / OpenedTempDirectory
                                  → TempFile / TempDirectory
```

The public resource remains bound to its originating `FileSystem`. Rooted
persist, cleanup, and child operations retain the original root authority.
Persist failure mapping uses both native axes; the effect column describes the
current target publication attempt rather than all historical publication:

| Native publication | Native source | `qubit-fs` state | `FsEffectState` |
| --- | --- | --- | --- |
| `NotPublished` | `Owned` | `NotPublished` | `Unchanged` |
| `NotPublished` | `Released` | `NotPublishedSourceReleased` | `Unchanged` |
| `NotPublished` | `Indeterminate` | `NotPublishedSourceIndeterminate` | `Unchanged` |
| `NotPublished` | `CleanupRequired` | `NotPublishedSourceCleanupRequired` | `Unchanged` |
| `Published` | `CleanupRequired` | `PublishedSourceRetained` | `Applied` |
| `Published` | `Released` | `PublishedSourceReleased` | `Applied` |
| `Published` | `Indeterminate` | `PublishedSourceIndeterminate` | `Applied` |
| `Indeterminate` | any | `Indeterminate` | `Indeterminate` |
| `Published` | `Owned` | invariant violation; `Indeterminate` | `Indeterminate` |

The adapter restores the retained native resource before constructing the
portable error. At the direct adapter boundary, a rejected invalid retry has an
`Unchanged` target effect for that call. The facade may retain an earlier
published recovery state and `publication_target` without invoking the adapter;
that historical snapshot is not a new publication. Source-indeterminate states
stay indeterminate across later path errors, cleanup failures, and asynchronous
cancellation in the portable facade.

Only `overwrite` and `creates_parent` map to `LocalPersistOptions`; unsupported
atomicity or metadata requirements are not fabricated. Explicit cleanup reports
errors, and `Drop` is best effort. When source qualification is indeterminate
but publication is known, the adapter uses the corresponding
source-indeterminate state and preserves the known target effect. Future unknown
variants and the invalid `Published` + `Owned` combination fall back
conservatively to `Indeterminate`.

Temporary-directory cleanup has native limits separate from ordinary
`delete_file`/`delete_directory` budgets. `LocalTempCleanupControl` is shared by
its clones and associated adapter directories; `None` means unbounded native
cleanup. Existing constructors (including the registry provider) keep their
unbounded behavior. New temporary directories receive the current limits at
creation; the adapter refreshes them again before `persist`, `keep`, explicit
`cleanup`, and `Drop`. A control update therefore affects subsequent adapter
operations that refresh those limits; it does not
interrupt an operation already using its native snapshot, and native internal
cleanup retries do not reread the shared control. Explicit cleanup failure
retains the handle for retry after changing the control, provided source
ownership is still known. A budget change cannot restore cleanup authority
when source ownership is indeterminate. Native `Drop` cleanup is best effort
and makes at most one further attempt; it provides neither guaranteed cleanup
nor one total budget across prior explicit attempts and `Drop`. See the
[English user-guide cleanup-budget section](user_guide.md#temporary-directory-cleanup-budgets).

Cleanup errors describe the effect of the current call. If cleanup removes an
Owned source and then fails to release its private sandbox, the error reports
`PartiallyApplied` and retains the sandbox-pending recovery state. A retry that
only releases that sandbox reports its own effect and does not claim that it
removed the source again. Explicit cleanup errors retain the resource handle so
the caller can adjust its cleanup control and retry when source ownership is
known.

## 11. Async boundary

Version 0.12.0 implements synchronous `FileSystemSpi` only: no
`AsyncFileSystemSpi`, boxed futures, runtime tasks, or automatic async registry
provider around blocking I/O. Async copy is verified by providers implementing
the async SPI.

## 12. Error mapping

Private functions in `spi/error_mapper.rs` centralize mapping. Native
`persist` and `keep` failures include their `LocalPersistStage` in the
diagnostic message. This text is for diagnostics, not a string-parsing API;
recovery uses typed state/effect, and the original `LocalFileError` remains the
direct source (copy failures retain their typed `LocalCopyFailure` source).
`map` and `map_without_path` attach the adapter's provider ID and available
logical request context; they do not derive identity from the native path.
Error-kind precedence is: preserve an explicit native `cause_kind()` for
`InvalidPath`, `InvalidOptions`, `InvalidState`, `NotDirectory`,
`IsDirectory`, and `TypeConflict`; otherwise map an attached I/O kind before
falling back to the native cause kind. Without a cause, `PublicationIncomplete`
maps to `Io` and `Indeterminate` remains `Indeterminate`. Effect state is
attached separately and never overrides the cause category. Requirement and
indeterminate states remain typed, and impossible adapter states use
`ProviderContractViolation`.
Impossible adapter states use `ProviderContractViolation`.

## 13. Registry integration

The `registry` feature provides `LocalFileSystemProvider`, the default
`local-file` identity and `file` alias, absolute local `file:` URI decoding, and
concrete `FileSystemResolution`. Remote authorities, unsupported
query/options/credentials, non-empty `FileSystemConfig` metadata, relative
paths, and non-`file` schemes are rejected. After the non-`file` scheme check
and URI structure parsing, options, metadata, and credentials are rejected
before provider-specific URI path decoding.
Unsupported schemes return `ProviderFailureKind::Unsupported` and may continue
under `OnAbsence`; recognized invalid `file:` requests and rooted
initialization failures are terminal. `rooted_with_descriptor` preserves the
caller descriptor and aliases. Rooted authority and `FileSystemId` are absent
from canonical URIs, so replay must persist them with provider selection.
Named selection chooses one provider; fallback requires an explicit chain, auto
selection, or registry default selection.

Host URI decoding validates that the path can be represented by the native
platform before constructing a resolution. Unix Host
`file:///reports/summary.csv` decodes to `/reports/summary.csv`; replay through
Host selects the `local-host` filesystem identity. On Windows, Host `file:` URIs
need an absolute drive path such as `file:///C:/reports/summary.csv`, which
decodes to `/C:/reports/summary.csv`; a drive-less path such as
`file:///tmp/data` is invalid. Rooted URIs such as
`file:///reports/summary.csv` decode inside the registered authority on both
platform families. Canonical URI replay must preserve the Rooted provider
selection/descriptor and filesystem identity because neither the Rooted
authority nor its identity is encoded in the URI.

The opt-in `registry` feature also submits one process-host provider to the
synchronous filesystem inventory. Its factory selects the finite
`LocalResourcePolicy::standard()` limits. Rooted providers retain caller-owned
authority and remain explicit registrations. Applications must link this crate
to include its inventory submission in the final binary.

## 14. Platform boundary

The adapter must not contain Unix/Windows business branches for copy, walking,
containment, symlinks, publication, or durability. Only representation-level
path differences remain here; operating-system logic belongs in
`qubit-local-files`. Linux and Windows are the adapter's regular behavioral-test
platforms; atomicity and durability are conditional capabilities subject to
native operation and filesystem support. FreeBSD is temporarily unsupported
by the native crate for important copy, rename, staged-write, and
temporary-publication paths. Reconsider support only after verifying safe Host
and Rooted no-replace primitives, recording filesystem and mount details for
metadata failures, resolving descriptor-exhaustion cleanup, and passing the
relevant native tests and doctests on a declared FreeBSD filesystem. See the
[native FreeBSD compatibility status](https://github.com/qubit-ltd/rs-local-files/blob/main/doc/freebsd-compatibility.md).

## 15. Module organization

```text
src/
├── constants.rs
├── local_copy_resource_limits.rs
├── local_delete_resource_limits.rs
├── local_directory_reopen_policy.rs
├── local_file_systems.rs
├── local_list_resource_limits.rs
├── local_resource_policy.rs
├── local_temp_cleanup_control.rs
├── local_temp_cleanup_resource_limits.rs
├── spi/
│   ├── local_file_system_spi.rs
│   ├── local_options_mapper.rs
│   ├── local_outcome_mapper.rs
│   ├── local_file_writer_spi.rs
│   ├── local_directory_stream_spi.rs
│   ├── local_range_reader.rs
│   ├── local_temp_resource_spi.rs
│   ├── local_temp_path_projection.rs
│   └── error_mapper.rs
├── path/local_path_mapper.rs
└── registry/
    ├── local_file_system_provider.rs
    └── local_file_uri_path.rs
```

There is one `LocalFileSystemSpi`; host/rooted behavior is selected by native
scope. Shared mapper/session logic stays in focused modules.

## 16. Verification strategy

1. **Mapping tests:** fake native outcomes/errors prove request, metadata,
   outcome, and failure-state mappings.
2. **Adapter integration tests:** host/rooted construction and identity,
   immutable properties snapshots (provider, identity, capabilities, limits,
   and path constraints), registry resolution, conversion-before-I/O,
   zero-side-effect copy rejection,
   native-copy no-fallback, typed copy/rename states, writer/temp terminal
   states, and rooted temporary authority.
3. **Provider contract tests:** the public facade runs the applicable
   `qubit-fs-testkit::FileSystemContractSuite`.

Native platform algorithms are tested by `qubit-local-files`; this crate checks
that translation preserves their semantics. The fuzz target is
`fuzz/fuzz_targets/registry_uri_resolution.rs`, bounds input to 4096 bytes, and
exercises URI resolution without filesystem mutation.

## Standard budgets and explicit overrides

`LocalResourcePolicy::standard()` performs no I/O and selects these finite
per-operation ceilings. It is an explicit constructor, not a `Default` implementation.

| Operation | Depth | Entries | Byte budget | Open directories | Deadline |
| --- | --- | --- | --- | --- | --- |
| list | 64 | 100,000 | 16 MiB path/name text | 32 | 30 seconds |
| copy | 64 | 100,000 | 1 GiB payload | 32 | 30 seconds |
| delete | 64 | 100,000 | 16 MiB pending path text | not separately configured | 30 seconds |

Use `with_list_limits(Some(...))`, `with_copy_limits(Some(...))`, or
`with_delete_limits(Some(...))` to replace one domain. Passing `None` explicitly
removes that domain's ceiling. Other domains remain unchanged. Deadlines are
cooperative, not interruption of blocked native calls; budgets are not process
RSS limits or aggregate concurrent-request quotas. Temporary-resource lifecycle
cleanup remains separate from ordinary deletion budgets.

Opening writer or temporary sessions follows the `qubit-fs` 0.4 `OpenFailure` contract.
Preserve its recovery session and any explicit cleanup error; see the
[core recovery guide](https://github.com/qubit-ltd/rs-fs/blob/main/doc/user_guide.md#opening-failures-and-recovery).

This provider advertises Conditional `RangeRead` for native regular-file windows.
The adapter wraps the already opened `LocalFileReader` in `LimitInput`; it does
not reopen the path. It seeks and reads through the same native handle, retaining
full resource metadata. After opening the resource, zero-length windows return
empty without seeking; missing paths still fail. EOF and beyond-EOF reads
normally return empty, but native seek or read errors still propagate. An
offset above the signed native position range is treated as empty only when
metadata from that same opened handle proves it is at or beyond EOF. Metadata
is an opening observation, not a snapshot under concurrent mutation.
Automatic `read_prefix` narrowing requires Guaranteed range support, so local
prefix reads continue to use bounded sequential consumption.

## Metadata contract

The public policy matrix is maintained in the [user guide](user_guide.md#metadata-preservation).
The adapter maps copy `None` to native `None` and `Portable` to native
`Permissions`; `UserMetadata`, `ProviderNative`, and `All` return
`RequirementNotMet` before native copy. Native permissions are the only
explicitly supported copy category. The current regression covers Unix mode
bits for the facade's portable copy path.

| Policy | Copy adapter | Persist adapter/native behavior |
| --- | --- | --- |
| `None` | Accepted; native `None`. | Accepted; no attribute clearing. |
| `Portable` | Accepted; native `Permissions` (Unix mode bits and Windows file readonly flag). | Accepted; same-object rename carries source permissions (Unix mode bits and Windows file readonly flag). |
| `UserMetadata` | `RequirementNotMet` before native copy. | Accepted; no user metadata setter or verification. |
| `ProviderNative` | `RequirementNotMet` before native copy. | Accepted; no provider-specific metadata operation. |
| `All` | `RequirementNotMet` before native copy. | Accepted; not an all-fields guarantee. |

The native permission operation is platform-specific; tests assert Unix mode
and, on Windows, the file readonly flag. Directory readonly is not used as an
ACL proxy; the Windows directory case sets and checks its child file's readonly
flag. Do not infer timestamp, owner, ACL, extended attribute, or user key/value
preservation from `Portable`.

Persist differs: `local_temp_resource_spi::persist_options` maps only overwrite
and parent-creation options. The native persist API has no metadata policy
setter. The facade still passes all five policy values, and behavior tests
exercise Host/Rooted file/directory cases for each value. Successful
publication moves the same temporary object: Host file uses
`move_file_without_replacing` or `replace_file`; Rooted file uses native rename
or rename-without-replacement; directories use native rename. There is no
copy-based fallback. Overwrite replaces the destination entry, so its former
attributes are not the attributes of the published source. Tests verify source
payload, removal of the temporary path, target reporting, and Unix source mode
for files and directories; the overwrite regression verifies source mode wins
over a different existing target mode.

Thus persist policy values are accepted but do not change native publication.
Successful rename carries the source object's permissions as a consequence of
moving that object; `None` does not request attribute clearing. The adapter
does not claim unchanged timestamps, owner, ACLs, extended attributes, or user
metadata, and the outcome has no field-level metadata receipt. A native move
failure remains a publish failure with the existing native recovery state; no
category is rejected merely because the adapter does not map it to a setter.
Metadata behavior does not imply durability.
