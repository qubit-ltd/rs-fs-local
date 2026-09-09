# qubit-fs-local 用户手册

[English](user_guide.md) · [README](../README.zh_CN.md) · [API 文档](https://docs.rs/qubit-fs-local)

## 手册目标与读者

本手册面向需要由本地主机支撑同步文件系统的 `qubit-fs` Rust 应用，覆盖当前
`qubit-fs-local` 0.7.0 版本（包版本 `0.7.0`）：直接创建 host/rooted 门面，以及可选的
registry provider。

## Provider 资源上限

原生 `LocalFileSystem` 默认 Options 是可被替换的便利配置；本适配层将
`LocalResourcePolicy` 作为每次请求的强制上限。list/copy 请求预算与 provider 预算
取更严格者，请求省略预算也不会移除 provider 上限。这不是跨并发请求的累计配额。

适配层先从完整请求构造操作行为，再调用原生 `tighten_resource_limits` 收紧预算。
provider 上限不会开启递归、忽略缺失目标、创建父目录或改变覆盖策略。writer 显式使用
原生 `PreserveExisting`，可移植 API 不增加原生元数据策略开关。临时资源发布把
命名空间绝对目标传给 `persist_with`。门面仍遵循 `qubit_fs::Path` 的逻辑规范化契约；
原生 Host 对 `link/..` 的保留不改变逻辑路径语法，也不会恢复已由可移植层折叠的组件。
需要原生点组件遍历时，直接使用 `qubit-local-files`。

`LocalResourcePolicy::bounded(list, copy, delete)` 为三类普通递归操作设置彼此独立的上限。
删除请求选择递归或忽略缺失时仍保留这些
上限。请求目录自身计为一个条目、深度为零；待处理路径按原生编码长度计费，不包括分配器
开销。期限采用协作式检查。构造 provider 时可用 `with_delete_limits(None)` 显式取消
删除上限；不会从 list/copy 预算推导隐藏的删除限制。

三类上限的计量对象不同：

| 上限 | 计量的工作 | 执行位置 |
| --- | --- | --- |
| provider list `max_entries` | adapter prefix 过滤前由 walker 产出的条目 | 原生 walker |
| request list `max_entries` | prefix 过滤后返回的条目 | 门面 |
| copy/delete 上限 | 对应操作的原生遍历和 payload 工作 | 原生操作 |

因此，即使 prefix 没有匹配项，也可能耗尽 provider walker 上限。协作式 deadline
会在原生调用前后检查，不能中断已经开始的 I/O 调用。

下面的数值是应用场景示例，不是库默认值：

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

逻辑路径文本上限报告为 `Unknown`，因为原生 component 上限无法涵盖百分号展开和非 UTF-8
文件名。绝对原生主机路径应使用 `host_path_to_logical` 转换；已经是 rooted 逻辑文本时使用
`qubit_fs::Path::parse`。临时资源 cleanup、Drop 和原生发布清理属于独立生命周期范围，不继承
普通删除上限。显式 cleanup 会报告自身错误；Drop 是 best effort。local provider 会接管所有
它能够表达的 copy，不会 Declined 到门面 fallback；原生工作开始后的失败保留状态和部分统计。

adapter 保留 native 的删除分类：通过 `delete_file` 删除目录返回 `IsDirectory`，通过
`delete_directory` 删除普通文件或最终符号链接返回 `NotDirectory`，且不会删除该 entry。
递归删除已经移除条目后失败时，应在重试前检查返回 `FsError` 的副作用状态。其 native
`LocalFileError` source 单独提供 `cause_kind()`；effect 缺失表示没有证据证明副作用。

## 概念模型

`LocalFileSystems` 是创建具体 `FileSystem` 门面的工厂。

```text
应用逻辑 Path
      │
      ├─ host() ──────────────► 进程主机命名空间
      └─ rooted*_with_id() ───► 一个保留的原生根目录
```

门面接受绝对、层级化的 `qubit_fs::Path`。rooted 门面保留一个原生根目录，并使用其下的
逻辑路径。`rooted` 创建进程本地 ID；`rooted_with_id` 使用给定的 `FileSystemId`。

启用 `registry` feature 后，`LocalFileSystemProvider` 会为支持的 `file:` 配置创建 provider
factory。成功的 registry resolution 会给出已配置的文件系统、provider 解码的逻辑路径和
canonical URI。

## 实战场景

某应用将生成的报表放在 `/srv/app-data` 下，且不应在每次操作时将该原生根目录当作应用
路径处理。成功标准是经由一个 rooted 门面以逻辑路径 `/reports/summary.csv` 访问文件。

## 安装与最小配置

```bash
cargo add qubit-fs qubit-fs-local
```

如需 registry，请启用 feature，并在应用中添加 registry crate：

```bash
cargo add qubit-fs-registry
cargo add qubit-fs-local --features registry
```

## 核心工作流

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

所有构造函数都需要 `LocalResourcePolicy`；普通递归工作流应传入完整的 bounded
list/copy/delete limits，只有明确接受无界递归资源使用时才用 `unbounded()`。只有当进程主机命名空间就是预期 authority 时才选择
`host(policy)`。当文件系统标识需要由应用提供时使用 `rooted_with_id`；进程内唯一标识足够时
使用 `rooted`。

## 进阶用法

在应用组装阶段注册本地 provider，以解析经过校验的 `file:` URI：

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

如改用 `LocalFileSystemProvider::rooted(id, root, policy)` 注册 provider，它会在构造阶段打开给定的
原生 authority，并返回 `FsResult<LocalFileSystemProvider>`。后续 resolution 会复用已打开的
authority，不会重新打开配置的根路径。打开 authority 失败时构造函数返回错误，因此不会注册
provider，也不会让它进入后续 fallback。便捷的 `rooted` 构造函数使用带有 `file` alias 的
默认 `local-file` descriptor。
如果同一个 registry 需要多个 rooted authority，请改用
`rooted_with_descriptor`，并为每个 provider 使用不同的 descriptor ID。该构造函数会原样保留
调用方传入的 descriptor（包括 aliases），不会自动添加默认的 `file` alias。

例如，两个 rooted provider 可以解析同一个逻辑路径，同时保留各自的原生 authority 和 identity：

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

两个 resolution 的 canonical URI 都是 `file:///reports/summary.csv`：其中只有 provider
解码后的逻辑路径，不携带 rooted 原生 authority。因此，单独使用 canonical URI 无法恢复
rooted provider 或 filesystem identity。若要持久化或 replay rooted resolution，必须将
provider selection/descriptor 和 filesystem identity 一并保存。只把 canonical URI 交给 host
provider replay 时，会得到相同的 URI 和逻辑路径，但文件系统会变成 `local-host`，不会回到
原来的任一 rooted filesystem。

## 错误与诊断

原生操作失败会经由 `qubit-fs` 错误模型报告；本地 adapter 的 provider ID 为 `local-file`。
registry 的创建和解析错误由 `qubit-fs-registry` 返回。应检查返回错误，而非假设 URI 已被接受。

provider 会拒绝超出本地文件契约的配置：远程 authority、query、相对路径、非 `file` scheme、
options 和 credentials。
它按 URI component 解码百分号编码的路径字节；编码后的分隔符和 NUL 字节会被拒绝，避免其
改变逻辑层级。

对于非 `file` scheme，provider 返回 `Unsupported`，错误 kind 为
`UnsupportedOperation`。使用 `FallbackPolicy::OnAbsence` 的 registry 可以继续尝试下一个
provider。scheme 已经是 `file` 后，远程 authority、query、options、credentials 或非法路径
等配置错误仍然是终止错误；`OnAbsence` 不会跳过这些错误。rooted authority 无法打开时也会
以终止性的 initialization failure 结束。

只有显式的 `ProviderSelection::chain`、自动的 `ProviderSelection::auto()`，或经由 registry
默认 selection 的解析才会启用 fallback。`FileSystemConfig` 未设置 selection 时，
`FileSystemRegistry::resolve_config` 会从 URI scheme 派生 named selection；named selection
只选择一个 provider，不会自动 fallback。

native `PublicationIncomplete` 会映射为 `FsEffectState::PartiallyApplied`，并将底层 native
错误保留为 source。原因与副作用彼此独立：副作用状态描述命名空间进度，映射后的错误类别
在可用时遵循 native cause。

## 排障

| 现象 | 检查项 |
| --- | --- |
| 无法使用 `LocalFileSystemProvider` | 启用 `registry` feature。 |
| `file:` URI 无法解析 | 确认它是绝对 URI、没有远程 authority 或 query，且未提供 options 或 credentials。 |
| rooted 门面无法打开 | 确认原生根目录存在并能被进程打开。 |
| 路径不可用 | 在所选门面的 authority 内使用绝对、层级化的 `qubit_fs::Path`。 |

## 限制与最佳实践

- 公共门面是同步的；本 crate 不提供异步本地文件系统门面。
- rooted containment 与原生文件系统行为属于 provider 边界；应保留 rooted 门面，而非在应用中反复拼接原生路径。
- 将 `file:` URI 视为仅本地输入。远程 authority 和 URI options 有意不作为该 provider 的配置通道。
- 如果需要 replay rooted resolution，应将 provider selection 和 filesystem identity 与 canonical URI 一起保存；URI 本身不能标识其保留的原生 authority。

## 延伸阅读

- [README](../README.zh_CN.md)
- [English user guide](user_guide.md)
- [API 文档](https://docs.rs/qubit-fs-local)

## 文件系统契约更新

列举时传入 `ListScope::Path(path)`；已配置的层级根目录使用
`ListScope::Path(Path::root())`。`ListScope::Namespace` 会在 native I/O 前被拒绝。
本地 provider 未声明 `RangeRead`，因此 `read_prefix` 继续有界顺序读取，不自动增加范围要求。
