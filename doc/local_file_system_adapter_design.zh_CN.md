# Qubit FS Local Adapter 设计

> 状态：已批准的目标设计，已按最终版 `qubit-fs` 与
> `qubit-local-files` 0.3 公共边界复核。本文定义 `qubit-fs-local` 重构后的职责和映射
> 契约；实现与回归测试以本文定义的公共边界和映射契约为收敛目标。
>
> 适用于 `qubit-fs-local` 0.8.0（包版本 `0.8.0`）与 `qubit-fs` 0.7 · [English design](local_file_system_adapter_design.md) ·
> [用户手册](user_guide.zh_CN.md)

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

Host 与 rooted 不对应两套 SPI 类型。`LocalFileSystemSpi` 内部持有已配置的
`native_files::LocalFileSystem`，由 native instance 决定 namespace。

## 4. 普通调用入口

应用通过仅作为类型命名空间的空枚举 `LocalFileSystems` 的关联方法创建门面：

```rust
pub enum LocalFileSystems {}

impl LocalFileSystems {
    pub fn host(policy: LocalResourcePolicy) -> FsResult<FileSystem>;

    pub fn rooted(
        root: &Path,
        policy: LocalResourcePolicy,
    ) -> FsResult<FileSystem>;

    pub fn rooted_with_id(
        id: FileSystemId,
        root: &Path,
        policy: LocalResourcePolicy,
    ) -> FsResult<FileSystem>;
}
```

`rooted` 为该 configured instance 生成稳定到其生命周期结束的 identity；
`rooted_with_id` 供 registry、持久配置和测试显式指定 identity。Identity 不从可能含
敏感信息的 native path 文本直接派生，也不通过含义不清的可选参数传入。

常见使用：

```rust
use qubit_fs::Path as LogicalPath;
use qubit_fs_local::LocalResourcePolicy;
use std::path::Path as NativePath;

let file_system = LocalFileSystems::rooted(
    NativePath::new("/data"),
    LocalResourcePolicy::unbounded(),
)?;
let path = LogicalPath::parse("/reports/summary.csv")?;
let metadata = file_system.stat(&path)?;
```

`LocalFileSystems` 只组织 factory，不包含文件算法。每个 factory 都要求显式 `LocalResourcePolicy`；
只有调用方明确选择时才允许无界递归资源使用。

## 5. SPI 实现

SPI 实现位于明确命名空间：

```rust
pub mod spi {
    pub struct LocalFileSystemSpi {
        native: native_files::LocalFileSystem,
        // configured local identity/codec data
    }
}
```

SPI 类型可以公开，便于 provider 开发与测试，但不在 crate 根部作为普通使用入口。

`LocalFileSystems::host` 和 `rooted` 都：

1. 构造 native filesystem/context；
2. 构造对应 SPI；
3. 调用 `FileSystem::from_spi`；
4. 返回已经校验 properties 快照的具体门面。

不返回 `Arc<dyn FileSystemSpi>`。

## 6. Properties

Local SPI 的 `properties()` 返回稳定、无 I/O 的 `ProviderProperties`，包括：

- `ProviderProperties::new` 中的 `FileSystemInfo`（configured `FileSystemId`、provider id
  和 `PathSemantics::Hierarchical`）；
- host-wide 或 rooted authority 描述；
- 从 native capability 映射出的 `FileSystemCapabilities`；
- 从平台和配置映射出的 `FileSystemLimits`；
- host/rooted namespace 对应的 `PathConstraints`；
- 经过安全校验的非敏感 diagnostics。

Capability 只声明当前平台和当前配置真正保证的语义。例如：

- no-replace atomic rename 只在 native 层证明支持时声明；
- rooted containment 只在使用可靠 descriptor/handle-relative 实现时声明；
- temp atomic persist 取决于同目录 staging 与 publication 能力；
- `SizeLimit::VariesByPath` 和 `SizeLimit::Unknown` 都保守映射为
  `FileSystemLimit::Unknown`，不使用猜测常量。

原生 `NAME_MAX`、`PATH_MAX` 等限制的单位和 authority 不等同于逻辑路径 UTF-8 文本长度，
百分号编码和非 UTF-8 文件名还会造成文本展开。因此 local provider 的
`max_path_text_bytes` 与 `max_component_text_bytes` 报告 `FileSystemLimit::Unknown`；它不
将 native 限制乘以猜测常量写入逻辑属性。解码后的路径仍由 codec、native 校验和操作系统
限制共同约束。

两个 local configured filesystem 都使用 hierarchical、absolute logical path：

- host 使用 platform-defined canonical absolute form，不能依赖调用时 current
  working directory；Unix `/` 表示 native root，Windows 第一版使用
  `/<drive>:/...` 且不把 `/` 伪装成跨 drive 的全局 root；
- rooted logical `/` 表示已打开的 native root authority；
- relative logical path、dot component 等稳定限制由 `PathConstraints` 在 adapter
  I/O 前拒绝；
- platform prefix/drive 与 canonical native text 的转换限制由
  `LocalPaths`/`LocalPathCodec` 在同一无副作用阶段执行；
- 运行时 mount、device、symlink 或 reparse 状态不能伪装成静态
  `PathConstraints`。

Properties 构造完成后不能因某次调用结果而动态改变。

## 7. Path 映射

### 7.1 Codec ownership

Adapter 的私有 `path::local_path_mapper` 不包含平台算法，而是直接委托
`qubit-local-files` 的 codec 和 `LocalPaths`：

```text
qubit_fs::Path canonical component text
  → `path::local_path_mapper::{native,logical}`
  → native_files::path::LocalPaths / LocalPathCodec
  → OsStr / OsString
```

Unix raw byte、Windows UTF-16/WTF-8、canonical escape、separator、root 和 prefix
判定全部由 `qubit-local-files` 提供。Adapter 只组织逻辑 component 与 native
component/path 的组合。

路径转换实际集中在 `path::local_path_mapper` 的私有 free functions
`native` 与 `logical` 中。公共 `host_path_to_logical` 是 host native 路径进入门面的唯一
推荐入口；它要求绝对 native 路径，并保留非 UTF-8 与百分号编码，不通过 lossy display
文本。Codec 或路径校验错误由私有 `path::local_path_mapper` 映射为无副作用的
`FsError`。

### 7.2 Host filesystem

Host SPI 将 absolute hierarchical `qubit_fs::Path` 逐 component 转换为 native
absolute path：

- `local_path_mapper::native` 把 scope 与 logical component iterator 交给
  `native_files::path::LocalPaths::from_canonical_components`；
- native 层统一处理 component codec、separator、root、drive、prefix 和 NUL；
- Windows 第一版只接受 drive-absolute canonical form；UNC/remote authority 在没有
  独立 provider authority 映射前拒绝；
- 不接受依赖 current working directory 的 relative logical path。

### 7.3 Rooted filesystem

Rooted 模式去除逻辑 absolute root，把剩余 component 转为相对于 native root 的安全
序列，交给 `native_files::path::LocalPaths::from_canonical_components`（`Rooted` scope），再调用
已通过 `native_files::LocalFileSystem::rooted` 配置的统一 native service。

Adapter 只执行 provider representation 转换；symlink/reparse containment、descriptor
traversal 和 race-sensitive 逻辑全部由 native 层完成。

### 7.4 多路径与返回路径

Copy、rename、persist 等多路径操作必须先成功转换本次调用涉及的全部逻辑路径和
options，再开始 metadata probe 或其他 native I/O。第二个路径转换失败时，第一个
路径不能已经触发副作用。

Provider 返回的 native entry/temp path 必须在离开 adapter 前通过
`native_files::path::LocalPaths::to_canonical_components`（传入对应 scope）编码回 canonical
component text，并构造
`qubit_fs::Path`。Native 诊断路径不能替换 provider-local identity，也不能进入
credential-free canonical URI。

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
| `CopyRequest` | 可选 `try_copy` → 统一 native `copy` |
| `RenameRequest` | `rename` |
| `CreateTempFileRequest` | `LocalTempFile` |
| `CreateTempDirectoryRequest` | `LocalTempDirectory` |

删除映射遵循 native 的类型契约：`delete_file` 遇到目录返回 `IsDirectory`，
`delete_directory` 遇到普通文件或最终符号链接返回 `NotDirectory`，且不删除该 entry。
递归删除已经移除条目后失败时，native `PublicationIncomplete` 通过
`LocalFileError::effect_state()` 映射为 `FsEffectState::PartiallyApplied`；底层原因仍由
`cause_kind()` 和 source 保留。普通错误没有足够副作用证据时 effect 为 `None`，adapter
不得把它推断成 `Unchanged`。

Adapter 不调用公开 options 的 `validate_against`，因为 SPI request 已代表完成的通用
preflight。Native 层仍可拒绝平台运行时条件。

所有 options conversion 集中在 `spi::local_options_mapper` 的私有 free functions 中。特别是
`WriteDisposition::{CreateNew, CreateOrReplace, Append}` 必须逐项映射到 native
disposition；只有 properties 声明 append 且 resolved atomicity 不是 `Required` 时
才能创建 native append writer。未知或不可表示的字段返回 contract error，不能静默
忽略。

### 8.1 Copy fast path

Local SPI 的 `try_copy` 采用两阶段规则：

1. 在零副作用阶段完成 source/target 全量路径转换，并把
   `ResolvedCopyOptions` 分类为 native local copy 可完整表达或不可表达；
2. 只有可完整表达时才调用已配置 native
   `LocalFileSystem` 实例的 `copy`。

结果映射如下：

```text
未进入 native copy，且本次组合不可表达
  → SpiCopyFailure(RequirementNotMet, Unchanged, zero partial stats)

native copy 成功
  → CopyAttempt::Completed(mapped outcome)

native copy 已被调用且失败
  → SpiCopyFailure(mapped state + partial stats)
```

不可表达的 options 可以在 codec、options classification 等确定无副作用的检查阶段被
拒绝；不得创建 staging、打开写 handle 或修改 namespace。local provider 不向门面
`Declined`，因此不会触发 facade stream fallback。一旦调用 native copy，任何
`LocalCopyFailure` 都是终止 failure，不能再按 I/O kind、`EXDEV` 或 `Unsupported` 转成
`Declined`。

Native local copy 内部使用流复制、clone 或跨设备 fallback，仍属于 provider-native
attempt。当前 `qubit-fs` 将 provider 已接管的这些原语统一表示为
`CopyMethod::Native`；“native”描述的是调用边界，不要求底层一定使用单次系统调用。
Adapter 仍按事实映射 atomicity、durability、metadata 和统计。

Native listing 返回的 path 已经属于请求的 namespace；Host 下层即使通过 alias 访问物理
目录，也不能在 adapter 中再次 canonicalize 或拼接物理前缀。Native copy 的 entry、byte、
depth 和 open-directory budgets 及其 `ResourceLimit`/partial stats 必须原样映射，不能为了
接受结果而放宽 provider contract。

`LocalCopyFailureState::{Unchanged, PartiallyPublished, Published, Indeterminate}` 与
partial stats 一一映射到 `SpiCopyFailure`。Native staging cleanup failure 保留为
typed source/安全 diagnostics，不能丢弃主 copy failure。

### 8.2 Rename primitive

`rename` 始终调用 native rename，不存在 copy+delete fallback。
`LocalRenameFailureState::{Unchanged, Renamed, Indeterminate}` 一一映射到
`SpiRenameFailure`。Native rename 完成但 parent durability 失败时必须保留
`Renamed`，不能压缩成普通 `FsError`。

## 9. Reader、writer 与 stream

### 9.1 Reader

Native reader 被包装为 `qubit_fs::spi::OpenedReader`：

- `OpenedFileInfo` 使用 properties 中的 `FileSystemId` 和请求对应的逻辑 `Path`；
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

公开 `TempFile` / `TempDirectory` 由 `FileSystem` 门面绑定，因此保留创建它的原始
filesystem 实例。Adapter 不重新调用 `LocalFileSystems::host()` 来伪造 owning
filesystem。

Host 模式委托 host native temp 入口；rooted 模式委托同一 native service
实例的 `create_temp_file` / `create_temp_directory`。Rooted session 持有 native rooted
temp handle，其
persist、cleanup 和 child 操作继续使用原 root authority，不能把 diagnostic host
path 当作 cleanup 权限。

Persist failure mapping 同时使用 native 的两个维度；effect 只描述本次目标发布，不能代替
历史发布事实：

| Native publication | Native source | `qubit-fs` state | `FsEffectState` |
| --- | --- | --- | --- |
| `NotPublished` | `Owned` | `NotPublished` | `Unchanged` |
| `NotPublished` | `Released` | `NotPublishedSourceReleased` | `Unchanged` |
| `NotPublished` | `Indeterminate` | `NotPublishedSourceIndeterminate` | `Unchanged` |
| `NotPublished` | `CleanupRequired` | `NotPublishedSourceCleanupRequired` | `Unchanged` |
| `Published` | `CleanupRequired` | `PublishedSourceRetained` | `Applied` |
| `Published` | `Released` | `PublishedSourceReleased` | `Applied` |
| `Published` | `Indeterminate` | `PublishedSourceIndeterminate` | `Applied` |
| `Indeterminate` | 任意 | `Indeterminate` | `Indeterminate` |
| `Published` | `Owned` | 内部不变量违例，映射为 `Indeterminate` | `Indeterminate` |

Adapter 先把原生错误持有的资源放回 session slot，再构造可移植错误。在直接 adapter
边界，非法重试被拒绝时，本次调用的目标 effect 为 `Unchanged`。门面可能不再调用 adapter，
而是继续返回此前的 published 恢复状态和 `publication_target`；这份历史快照不表示发生了
一次新发布。源资格不确定后，后续路径错误、cleanup 失败以及可移植异步门面的取消都不能
恢复源所有权。

`ResolvedPersistOptions` 只有 `overwrite` 和 `creates_parent` 会映射到
`LocalPersistOptions`；其他持久化要求不在 native persist 选项中伪造，native durability
使用 provider 明确声明的默认 requirement。

普通 `delete_file`/`delete_directory` 的预算不自动应用于 temporary session 的
`cleanup`、Drop 或 publication 失败后的 native 清理。cleanup 仍可能执行同步递归工作；
显式 cleanup 报告错误，Drop 是 best effort。需要对清理工作设预算时，应在 native 层另立
包含残留资源和恢复权属的设计，不能把普通删除预算解释为全部生命周期操作的总配额。

源资格不确定而目标事实明确时，必须使用对应的 `*SourceIndeterminate` 状态并保留已知
目标 effect。未来未知变体及无效的 `Published` + `Owned` 组合保守映射为整体
`Indeterminate`。

## 11. 异步边界

本轮 local adapter 只实现同步 `FileSystemSpi`。在 `qubit-local-files` 提供真正的
nonblocking/async 本地原语之前：

- 不实现 `AsyncFileSystemSpi`；
- 不用 `async fn`、boxed future 或 runtime task 包装 blocking `std::fs` I/O；
- registry feature 不把同步 local provider 自动注册成异步 provider；
- testkit 只对同步 local facade 运行 provider contract suite。

这不影响 `qubit-fs` 的 `AsyncCopyOperation` 契约；它由真正实现
`AsyncFileSystemSpi` 的 provider 验证。

## 12. Error 映射

错误映射集中在 `spi/error_mapper.rs` 的私有 free functions 中，不把 mapper 暴露为应用 API。
`map` 与 `map_without_path` 补齐 canonical provider、逻辑路径和 target context；copy
failure 通过独立的 typed `LocalCopyFailure` 映射路径保留完整状态。host 与 rooted SPI
共享这组映射函数。

映射规则：

- 有 typed I/O source 时优先按 `std::io::ErrorKind` 映射到最精确的 `FsErrorKind`；无
  source 时才回退到 native kind；`PathCodec` 失败保持 `InvalidPath`；
- `InvalidPath`/`InvalidOptions` 与 `NotDirectory`/`IsDirectory` 保持各自分类，不折叠为
  `Conflict` 或普通 I/O；
- public operation、source path 和 target path 取自 request；provider id 使用 adapter
  属性声明的 provider descriptor（host 默认是 `local-file`，rooted provider 可使用
  自定义 descriptor identity）；
- native path 仅在确认不会越过安全边界时进入 message；
- `std::io::Error` 保留为 source，不自动格式化；copy failure 还保留完整的
  `LocalCopyFailure`，因此 staging path 与 cleanup error 可通过 typed source 诊断；
- native requirement failure 映射为 `RequirementNotMet`；
- native indeterminate 映射为 `Indeterminate`；
- native `cause_kind()` 优先决定基础 `FsErrorKind`，`effect_state()` 仅补充已知的
  `FsEffectState`，两者不能互相覆盖；
- adapter 自身产生不可能状态时使用 `ProviderContractViolation`，不伪装成 I/O。

门面会补齐并规范化通用上下文，adapter 不伪造其他 provider identity。

## 13. Registry 集成

`registry` feature 提供 `LocalFileSystemProvider`。它负责：

- 声明默认的 canonical `local-file` provider identity，并注册 `file` scheme alias；
- `rooted` 使用上述默认 descriptor；`rooted_with_descriptor` 原样保留调用方 descriptor
  及其 aliases，不自动添加 `file` alias；rooted authority 在 provider 构造阶段打开，打开
  失败时不会产生可注册或参与 fallback 的 provider；
- 从受控 `ConnectionUri` 配置边界读取原始输入；
- 对非 `file` scheme 直接返回 `ProviderFailureKind::Unsupported`，其错误 kind 为
  `FsErrorKind::UnsupportedOperation`；在 `FallbackPolicy::OnAbsence` 下，registry
  可以继续尝试后续 provider；
- 接受无 authority 或空 authority 的 `file:` URI；
- 拒绝 remote authority、未支持 query 和 secret；
- provider-specific 解码 URI path；
- 将等价的绝对 `file:` 输入重新编码为唯一的 canonical URI（空 authority、规范化
  percent escape）；
- 根据配置选择 host 或 rooted filesystem；
- rooted provider 在构造阶段打开并保留 descriptor-backed authority，resolution 只 clone
  已打开的 `FileSystem`；
- 在凭据/敏感 query 已被移除后构造 canonical `Uri`；
- 返回具体 `FileSystemResolution`。

Provider 返回的是 `FileSystem` 门面，不是 `Arc<dyn FileSystem>` 或 operation SPI。

非 `file` 请求属于 provider 不适用的情况，返回 `Unsupported`，因此可以在
`OnAbsence` fallback chain 中交给其他 provider。已经识别为 `file` 的请求如果包含
remote authority、query、options、credentials 或非法路径，则返回
`InvalidConfiguration`（或对应的 `InitializationFailed`），不能借助 `OnAbsence`
跳过；rooted authority 无法打开也会终止解析。

Fallback 只由显式的 `chain`、`Auto` 或 registry 默认 selection 提供。`FileSystemConfig` 未
设置 selection 时，registry 会从 URI scheme 派生 `Named` selection；该 selection 只调用一个
provider，不会自动 fallback。

Rooted provider 的 native authority 不写入 canonical URI。比如两个 provider 分别保留
`/srv/tenant-a` 和 `/srv/tenant-b`，都将 URI `file:///reports/summary.csv` 解码为逻辑
路径 `/reports/summary.csv`，并返回相同的 canonical URI；它们的 provider descriptor 和
`FileSystemId` 仍然不同。canonical URI 只表示 provider 解码后的路径，不能单独恢复
rooted authority 或 filesystem identity。

因此，持久化或 replay rooted resolution 时必须同时保存 provider selection/descriptor
和 filesystem identity；只 replay canonical URI 会按当时选择的 provider 重新解析。若
使用默认 host provider replay，它会得到相同 URI 和逻辑路径，但得到的是 `local-host`
identity，不能回到原来的 rooted filesystem。

Registry feature 只增加配置/解析适配，不把 registry 依赖带入默认 native 使用路径。

## 14. 平台边界

生产 adapter 源码不应包含 copy、walk、root containment、symlink、publication 或
durability 的 `cfg(unix)` / `cfg(windows)` 分支。

允许的少量平台差异仅限 representation adapter，例如选择已由
`qubit-local-files` 提供的 path codec。只要出现操作系统业务判断，就应下沉到
`qubit-local-files`。

## 15. 模块组织

```text
src/
├── constants.rs
├── local_copy_resource_limits.rs
├── local_delete_resource_limits.rs
├── local_directory_reopen_policy.rs
├── local_file_systems.rs
├── local_list_resource_limits.rs
├── local_resource_policy.rs
├── spi/
│   ├── local_file_system_spi.rs
│   ├── local_options_mapper.rs
│   ├── local_outcome_mapper.rs
│   ├── local_file_writer_spi.rs
│   ├── local_directory_stream_spi.rs
│   ├── local_temp_resource_spi.rs
│   └── error_mapper.rs
├── path/
│   └── local_path_mapper.rs
└── registry/
    ├── local_file_system_provider.rs
    └── local_file_uri_path.rs
```

当前实现只有一个 `LocalFileSystemSpi`；Host/rooted 由其持有的 native scope 区分，
没有 `rooted_local_file_system_spi.rs`。共享转换逻辑进入实际的私有 mapper/session 模块，
不能合并回一个包含平台算法的巨大 adapter 文件。

## 16. 验证策略

测试分三层：

1. Mapping tests

   使用 fake native outcome/error 验证 request、metadata、outcome 和 failure state
   一一映射。

2. Adapter integration tests

   验证 host/rooted factory 返回 `FileSystem`，opened/temp handle 保留正确
   filesystem identity，properties 快照包含正确的 provider、identity、capability、limit
   和 path constraint，registry 返回 concrete resolution。

   另外覆盖全部输入路径转换发生在 I/O 前、不可表达 copy 的零副作用终止失败、native copy
   failure 不 fallback、copy/rename typed state 与 partial stats 无损映射，以及
   rooted temp 在 root 诊断路径变化后仍使用原 authority。

3. Provider contract tests

   使用 `qubit-fs-testkit::FileSystemContractSuite` 通过公开门面运行完整适用契约。

平台安全算法不在本 crate 重复测试；它们由 `qubit-local-files` 测试。本 crate 只验证
没有因转换丢失其语义。`fuzz/fuzz_targets/registry_uri_resolution.rs` 将输入限制为
4096 字节，在不修改文件系统的条件下验证 URI resolution。

## 标准预算与显式覆盖

`LocalResourcePolicy::standard()` 不执行 I/O，并为每次操作设置下表中的有限上限。
它是显式构造方法，不提供隐式 `Default` 实现。

| 操作 | 深度 | 条目数 | 字节预算 | 打开的目录数 | 期限 |
| --- | --- | --- | --- | --- | --- |
| list | 64 | 100,000 | 16 MiB 路径／名称文本 | 32 | 30 秒 |
| copy | 64 | 100,000 | 1 GiB payload | 32 | 30 秒 |
| delete | 64 | 100,000 | 16 MiB 待处理路径文本 | 不单独配置 | 30 秒 |

用 `with_list_limits(Some(...))`、`with_copy_limits(Some(...))` 或
`with_delete_limits(Some(...))` 覆盖对应领域；传入 `None` 表示显式取消该领域的上限，
不改变其他操作预算。期限采用协作式检查，不能打断阻塞的原生调用；预算不等于进程 RSS
上限，也不是并发请求的累计配额。临时资源的生命周期清理仍独立于普通删除预算。

writer 和临时会话的打开遵循核心 0.6 的 `OpenFailure` 契约。应保留恢复会话与显式清理
错误，具体见[核心恢复指南](https://github.com/qubit-ltd/rs-fs/blob/main/doc/user_guide.zh_CN.md)。

本地 provider 为原生普通文件窗口声明 Conditional `RangeRead`。适配层在同一个已打开
句柄上 seek 和读取，metadata 仍描述完整资源。零长度、EOF 和超出 EOF 的窗口在确认
资源存在后返回空；不存在的路径仍报错。并发修改时不承诺内容快照。自动前缀范围优化要求
Guaranteed 能力，因此本地 `read_prefix` 仍使用有界顺序消费。
