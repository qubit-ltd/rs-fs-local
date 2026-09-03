# qubit-fs-local 用户手册

[English](user_guide.md) · [README](../README.zh_CN.md) · [API 文档](https://docs.rs/qubit-fs-local)

## 手册目标与读者

本手册面向需要由本地主机支撑同步文件系统的 `qubit-fs` Rust 应用，覆盖当前
`qubit-fs-local` 0.1 版本：直接创建 host/rooted 门面，以及可选的 registry provider。

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

所有构造函数都需要 `LocalResourcePolicy`；只有明确接受无界递归资源使用时才用 `unbounded()`，
否则传入完整的 bounded list/copy limits。只有当进程主机命名空间就是预期 authority 时才选择
`host(policy)`。当文件系统标识需要由应用提供时使用 `rooted_with_id`；进程内唯一标识足够时
使用 `rooted`。

## 进阶用法

在应用组装阶段注册本地 provider，以解析经过校验的 `file:` URI：

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

如改用 `LocalFileSystemProvider::rooted(id, root, policy)` 注册 provider，它会在构造阶段打开给定的
原生 authority，并返回 `FsResult<LocalFileSystemProvider>`。后续 resolution 会复用已打开的
authority，不会重新打开配置的根路径。
如果同一个 registry 需要多个 rooted authority，请改用
`rooted_with_descriptor`，并为每个 provider 使用不同的 descriptor ID。

## 错误与诊断

原生操作失败会经由 `qubit-fs` 错误模型报告；本地 adapter 的 provider ID 为 `local-file`。
registry 的创建和解析错误由 `qubit-fs-registry` 返回。应检查返回错误，而非假设 URI 已被接受。

provider 会拒绝超出本地文件契约的配置：远程 authority、query、相对路径、非 `file` scheme、
options 和 credentials。
它按 URI component 解码百分号编码的路径字节；编码后的分隔符和 NUL 字节会被拒绝，避免其
改变逻辑层级。

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

## 延伸阅读

- [README](../README.zh_CN.md)
- [English user guide](user_guide.md)
- [API 文档](https://docs.rs/qubit-fs-local)
