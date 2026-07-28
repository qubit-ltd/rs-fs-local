# Qubit FS Local Adapter 设计

> 状态：已批准的目标设计。本文定义 `qubit-fs-local` 重构后的职责和映射契约；
> 当前实现迁移前可能与本文不同。

## 1. 定位

`qubit-fs-local` 是 `qubit-fs` 与 `qubit-local-files` 之间的薄适配层：

```text
qubit-fs 门面与 SPI 契约
            ▲
            │ implements FileSystemSpi
      qubit-fs-local
            │ delegates
            ▼
qubit-local-files 本地语义与平台实现
```

本 crate 不实现第二套本地文件系统算法。它只负责类型、路径、错误、结果和 session
之间的翻译。

## 2. 目标与非目标

目标：

1. 提供可直接使用的 local `FileSystem` 门面；
2. 实现 `qubit_fs::spi::FileSystemSpi`；
3. 将所有 native 业务逻辑委托给 `qubit-local-files`；
4. 准确声明 local configured filesystem 的 properties；
5. 把 local outcome 和部分成功状态无损映射到 provider-neutral 类型；
6. 可选集成 `qubit-fs-registry` 的 `file:` provider；
7. 不向普通应用暴露 operation SPI。

非目标：

- 实现 copy、walk、temp、publication、durability 或 root guard；
- 复制 Unix/Windows 条件分支；
- 重新校验门面已经保证的通用 request；
- 让 `qubit-local-files` 依赖 `qubit-fs`；
- 为 async API 包装 blocking local I/O；
- 在 adapter 中引入 runtime。

## 3. 依赖与命名

```rust
use qubit_local_files as native_files;
```

不为 `qubit-local-files` 类型增加 `Native` 前缀。三个层次通过完整路径区分：

```text
qubit_fs::FileSystem
qubit_fs_local::spi::LocalFileSystemSpi
qubit_local_files::LocalFileSystem
```

Rooted 类型同理：

```text
qubit_fs_local::spi::RootedLocalFileSystemSpi
qubit_local_files::RootedLocalFileSystem
```

## 4. 普通调用入口

应用通过零变体 enum 的关联方法创建门面：

```rust
pub enum LocalFileSystems {}

impl LocalFileSystems {
    pub fn host() -> FsResult<FileSystem>;

    pub fn rooted(
        root: &Path,
    ) -> FsResult<FileSystem>;

    pub fn rooted_with_id(
        id: FileSystemId,
        root: &Path,
    ) -> FsResult<FileSystem>;
}
```

`rooted` 为该 configured instance 生成稳定到其生命周期结束的 identity；
`rooted_with_id` 供 registry、持久配置和测试显式指定 identity。Identity 不从可能含
敏感信息的 native path 文本直接派生，也不通过含义不清的可选参数传入。

常见使用：

```rust
let file_system = LocalFileSystems::rooted(Path::new("/data"))?;
let path = FsPath::parse("/reports/summary.csv")?;
let resource = file_system.resource(path)?;
```

`LocalFileSystems` 只组织 factory，不包含文件算法。

## 5. SPI 实现

SPI 实现位于明确命名空间：

```rust
pub mod spi {
    pub struct LocalFileSystemSpi {
        // configured local identity/codec data
    }

    pub struct RootedLocalFileSystemSpi {
        native: native_files::RootedLocalFileSystem,
        // configured local identity/codec data
    }
}
```

SPI 类型可以公开，便于 provider 开发与测试，但不在 crate 根部作为普通使用入口。

`LocalFileSystems::host` 和 `rooted` 分别：

1. 构造 native filesystem/context；
2. 构造对应 SPI；
3. 调用 `FileSystem::from_spi`；
4. 返回已经校验 properties 快照的具体门面。

不返回 `Arc<dyn FileSystemSpi>`。

## 6. Properties

Local SPI 的 `properties()` 返回稳定、无 I/O 的 `FileSystemProperties`，包括：

- configured `FileSystemId`；
- provider id；
- `PathSemantics::Hierarchical`；
- host-wide 或 rooted authority 描述；
- 从 native capability 映射出的 `FileSystemCapabilities`；
- 从平台和配置映射出的 `FileSystemLimits`；
- 经过安全校验的非敏感 diagnostics。

Capability 只声明当前平台和当前配置真正保证的语义。例如：

- no-replace atomic rename 只在 native 层证明支持时声明；
- rooted containment 只在使用可靠 descriptor/handle-relative 实现时声明；
- temp atomic persist 取决于同目录 staging 与 publication 能力；
- unknown native path limit 仍表示 unknown，不使用猜测常量。

Native limit 只有在 `LocalPathLimit` 的单位经 codec 可证明地换算为 provider-local
`FsPath` text bytes 时才写入 `FileSystemLimits`；UTF-16 code units 不直接当 byte。

Properties 构造完成后不能因某次调用结果而动态改变。

## 7. Path 映射

### 7.1 Host filesystem

Host SPI 将 hierarchical `FsPath` 逐 component 转换为 native path：

- Unix 使用 `OsStrPathCodec` 保留支持的 native byte；
- Windows 使用对应 native code-unit codec；
- 拒绝 component 解码后引入 separator、root、prefix 或 NUL；
- host filesystem 只接受符合其 authority 规则的路径；
- relative/absolute 规则由 configured path model 明确，不由平台默认隐式决定。

### 7.2 Rooted filesystem

Rooted SPI 把 `FsPath` 转为相对于 native root 的安全 component 序列，再调用
`native_files::RootedLocalFileSystem`。

Adapter 只执行 provider representation 转换；symlink/reparse containment、descriptor
traversal 和 race-sensitive 逻辑全部由 native 层完成。

### 7.3 Location

Provider 返回的 opened path 必须映射回请求的 `FsPath`。Native 诊断路径不能替换
provider-local identity，也不能进入 credential-free canonical URI。

## 8. 操作映射

每个 SPI 方法只执行“读取 request、调用 native、映射结果”：

| `qubit_fs::spi` | `qubit-local-files` |
| --- | --- |
| `StatRequest` | `metadata` |
| `ListRequest` | `LocalDirectoryWalker` 或直接目录 stream |
| `OpenReaderRequest` | `open_reader` |
| `OpenWriterRequest` | `LocalFileWriter` |
| `CreateDirectoryRequest` | `create_directory` |
| `DeleteFileRequest` | `delete_file` |
| `DeleteDirectoryRequest` | `delete_directory` |
| `CopyRequest` | 统一 `copy` |
| `RenameRequest` | `rename` |
| `CreateTempFileRequest` | `LocalTempFile` |
| `CreateTempDirectoryRequest` | `LocalTempDirectory` |

Adapter 不调用公开 options 的 `validate_against`，因为 SPI request 已代表完成的通用
preflight。Native 层仍可拒绝平台运行时条件。

所有 options conversion 集中在私有零变体 `LocalOptionsMapper` 的关联方法中。特别是
`WriteDisposition::{CreateNew, CreateOrReplace, Append}` 必须逐项映射到 native
disposition；只有 properties 声明 append 且 resolved atomicity 不是 `Required` 时
才能创建 native append writer。未知或不可表示的字段返回 contract error，不能静默
忽略。

## 9. Reader、writer 与 stream

### 9.1 Reader

Native reader 被包装为 `qubit_fs::spi::OpenedReader`：

- `OpenedFileInfo` 使用请求对应的 provider-local `FileLocation`；
- 只有 native open 已经取得 metadata 时才附带 snapshot；
- 不为补齐 metadata 额外执行 `stat`。

### 9.2 Writer

定义 adapter session：

```text
native_files::LocalFileWriter
  → qubit_fs_local::spi 内部 writer adapter
  → implements qubit_fs::spi::FileWriterSpi
```

映射必须保留：

- write/flush I/O error；
- actual publication method；
- actual atomicity 与 durability；
- `RetryableNotPublished`；
- `NotPublished`；
- `Published`；
- `Indeterminate`。

Adapter 不自行执行 rename、fsync 或 staging cleanup。

### 9.3 Directory stream

Native lazy walker/stream 被包装为 `DirectoryStreamSpi`。Adapter 把每个 native entry
转换为 `DirEntry`，公开门面随后再次验证 namespace contract。

不把整个目录预加载进 `Vec`，不向上暴露 native continuation 或 directory handle。

## 10. Temporary resource

Native temp handle 被 adapter session 持有，并实现 `TempResourceSpi`：

```text
LocalTempFile / LocalTempDirectory
  → local temp session adapter
  → OpenedTempFile / OpenedTempDirectory
  → TempFile / TempDirectory
```

公开 temporary resource 的 `FileResource` 由 `FileSystem` 门面绑定，因此保留创建它的
原始 filesystem 实例。Adapter 不重新调用 `LocalFileSystems::host()` 来伪造 owning
filesystem。

Persist state 一一映射：

| Native state | `qubit-fs` state |
| --- | --- |
| `NotPublished` | `NotPublished` |
| `Published` | `Published` |
| `PublishedSourceRetained` | `PublishedSourceRetained` |
| `Indeterminate` | `Indeterminate` |

`ResolvedPersistOptions` 的 overwrite、atomicity 和 metadata preservation 映射到
`LocalPersistOptions`；native durability 使用 provider 明确声明的默认 requirement。

任何无法确定 source/target 状态的 native failure 都必须映射为 `Indeterminate`。

## 11. Error 映射

错误映射集中在一个私有 mapper 类型中，不使用散落的 free function：

```rust
struct LocalFileErrorMapper {
    properties: Arc<FileSystemProperties>,
}

impl LocalFileErrorMapper {
    fn map_error(
        &self,
        request: &impl LocalRequestContext,
        error: native_files::LocalFileError,
    ) -> FsError;
}
```

Host 与 rooted SPI 共享 `LocalFileErrorMapper`，但不把 mapper 暴露为应用 API。

映射规则：

- native kind 映射到最精确的 `FsErrorKind`；
- `InvalidPath`/`InvalidOptions` 与 `NotDirectory`/`IsDirectory` 保持各自分类，不折叠为
  `Conflict` 或普通 I/O；
- public operation、provider id、source path 和 target path 取自 request/属性快照；
- native path 仅在确认不会越过安全边界时进入 message；
- `std::io::Error` 保留为 source，不自动格式化；
- native requirement failure 映射为 `RequirementNotMet`；
- native indeterminate 映射为 `Indeterminate`；
- adapter 自身产生不可能状态时使用 `ProviderContractViolation`，不伪装成 I/O。

门面会补齐并规范化通用上下文，adapter 不伪造其他 provider identity。

## 12. Registry 集成

`registry` feature 提供 `LocalFileSystemProvider`。它负责：

- 声明 `file` provider identity 和 aliases；
- 接受无 authority 或空 authority 的 `file:` URI；
- 拒绝 remote authority、未支持 query 和 secret；
- provider-specific 解码 URI path；
- 根据配置选择 host 或 rooted filesystem；
- 返回具体 `FileSystemResolution`。

Provider 返回的是 `FileSystem` 门面，不是 `Arc<dyn FileSystem>` 或 operation SPI。

Registry feature 只增加配置/解析适配，不把 registry 依赖带入默认 native 使用路径。

## 13. 平台边界

生产 adapter 源码不应包含 copy、walk、root containment、symlink、publication 或
durability 的 `cfg(unix)` / `cfg(windows)` 分支。

允许的少量平台差异仅限 representation adapter，例如选择已由
`qubit-local-files` 提供的 path codec。只要出现操作系统业务判断，就应下沉到
`qubit-local-files`。

## 14. 模块组织

```text
src/
├── local_file_systems.rs
├── spi/
│   ├── local_file_system_spi.rs
│   ├── rooted_local_file_system_spi.rs
│   ├── local_file_writer_spi.rs
│   ├── local_directory_stream_spi.rs
│   ├── local_temp_resource_spi.rs
│   └── error_mapper.rs
├── path/
│   └── local_path_mapper.rs
└── registry/
    └── local_file_system_provider.rs
```

重构以此模块布局为目标。共享转换逻辑只能进入列出的私有 mapper/session 模块，不能
合并回一个包含平台算法的巨大 adapter 文件。

## 15. 验证策略

测试分三层：

1. Mapping tests

   使用 fake native outcome/error 验证 request、metadata、outcome 和 failure state
   一一映射。

2. Adapter integration tests

   验证 host/rooted factory 返回 `FileSystem`，resource 和 temp 保留同一门面实例，
   registry 返回 concrete resolution。

3. Provider contract tests

   使用 `qubit-fs-testkit::FileSystemContractSuite` 通过公开门面运行完整适用契约。

平台安全算法不在本 crate 重复测试；它们由 `qubit-local-files` 测试。本 crate 只验证
没有因转换丢失其语义。
