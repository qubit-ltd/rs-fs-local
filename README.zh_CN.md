# qubit-fs-local

[![Rust CI](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-fs-local/coverage-badge.json)](https://qubit-ltd.github.io/rs-fs-local/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-fs-local.svg?color=blue)](https://crates.io/crates/qubit-fs-local)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

`qubit-fs-local` 为 `qubit-fs` 应用提供同步的本地 `file:` 后端。当应用需要访问进程
主机文件系统，或将一个原生目录保留为 rooted 文件系统 authority，同时不希望在应用代码中
处理 URI 解析和原生路径转换时，可使用本 crate。

本文档适用于 `qubit-fs-local` 0.5.0（包版本 `0.5.0`）。

## 安装

```bash
cargo add qubit-fs qubit-fs-local
```

仅当需要通过 `qubit-fs-registry` 注册可选的 `file` provider 时，才启用 registry 集成：

```bash
cargo add qubit-fs-registry
cargo add qubit-fs-local --features registry
```

## 快速开始

如果应用必须将报表保留在 `/srv/app-data` 下，请以应用的稳定文件系统标识创建 rooted
门面，并在该 authority 内使用绝对逻辑路径：

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

所有构造函数都要求显式传入 `LocalResourcePolicy`。通常应使用包含三类操作预算的
`bounded(list, copy, delete)`；只有应用明确接受无界递归工作时才使用 `unbounded()`。
`LocalFileSystems::host(policy)`
打开进程主机命名空间；`rooted(root, policy)` 生成进程本地标识；`rooted_with_id(id, root, policy)`
保留调用方提供的标识。若该标识必须在进程之外保持稳定，应使用后者。

当原生主机路径来自 `std::env::current_dir` 等 API 时，应先使用
`host_path_to_logical` 转换，再传给门面。这样可以保留百分号转义和 Unix 非 UTF-8 文件名。
已经是 rooted 逻辑路径时使用 `qubit_fs::Path::parse`；两者表示的 authority 不同。

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

`bounded(list, copy, delete)` 显式设置三类普通操作的每次请求上限。list 和 copy 各自包含
深度、条目数、字节数/名称字节数、打开目录数和 deadline；delete 则包含深度、条目数、待处理
路径字节数和 deadline。列表的 provider 条目计数为 native walker 在 prefix 过滤前产出的条目；
请求条目上限计数为过滤后返回的条目。临时资源
cleanup、Drop 和原生发布清理不继承普通删除预算；这些配置也不是并发请求的累计配额。

## 提供的能力

- 通过 host 和 rooted 构造路径提供本地文件的具体同步 `FileSystem` 门面。
- 可选的 `LocalFileSystemProvider` 位于 `registry` feature 后；它将支持的 `file:` 配置
  解析为文件系统、路径和 canonical URI。
- provider 仅接受绝对 `file:` URI；会拒绝远程 authority、query、相对路径、非 `file`
  scheme，以及配置 options 或 credentials。
- rooted provider 会在构造阶段打开原生 authority。打开失败时构造函数返回错误，不会产生可
  注册或参与 fallback 的 provider。默认 `rooted` descriptor 是带有 `file` alias 的
  `local-file`；`rooted_with_descriptor` 原样保留调用方传入的 descriptor 及其 aliases。
- 如果同一个 registry 需要注册多个 rooted authority，请使用
  `LocalFileSystemProvider::rooted_with_descriptor`，每个 descriptor ID 会成为其文件系统的
  provider identity。

只有显式的 provider `chain`、自动的 `ProviderSelection::auto()`，或 registry 默认 selection
才会启用 fallback。`FileSystemConfig` 未设置 selection 时，`resolve_config` 会从 URI scheme
派生一个 named selection；该单 provider selection 不会自动 fallback。在
`FallbackPolicy::OnAbsence` 下，`file:` 配置错误仍然是终止错误，unsupported scheme 才可能
继续交给下一个 provider。

当前版本仅提供同步本地文件系统门面，不提供异步本地文件系统门面。

## 延伸阅读

- [English user guide](doc/user_guide.md)
- [中文用户手册](doc/user_guide.zh_CN.md)
- [English adapter design](doc/local_file_system_adapter_design.md)
- [中文适配器设计](doc/local_file_system_adapter_design.zh_CN.md)
- [API 文档](https://docs.rs/qubit-fs-local)
- [English README](README.md)

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./ci-check.sh

# 检查代码覆盖率
./coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./align-ci.sh`格式化代码，运行`./ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-fs-local](https://github.com/qubit-ltd/rs-fs-local)
