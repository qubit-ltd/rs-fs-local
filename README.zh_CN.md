# qubit-fs-local

[![Rust CI](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-fs-local/coverage-badge.json)](https://qubit-ltd.github.io/rs-fs-local/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-fs-local.svg?color=blue)](https://crates.io/crates/qubit-fs-local)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

`qubit-fs-local` 为
[`qubit-fs`](https://crates.io/crates/qubit-fs) 提供同步的主机本地 `file:`
文件系统后端。`LocalFileSystems` 可创建具体的主机范围或目录 authority 受限的
文件系统门面。通过 `file` provider 别名进行 registry 集成的能力位于可选的
`registry` feature 中。

## 安装

将本 crate 添加到项目。只有应用通过 `qubit-fs-registry` 使用
`LocalFileSystemProvider` 时才启用 `registry`：

```bash
cargo add qubit-fs-local
cargo add qubit-fs-local --features registry
```

## 使用方法

创建主机范围门面，或使用显式稳定 identity 打开一个 rooted authority：

```rust
use std::path::Path;

use qubit_fs::{FileSystemId, Path as LogicalPath};
use qubit_fs_local::LocalFileSystems;

let file_system = LocalFileSystems::rooted_with_id(
    FileSystemId::new("app-data")?,
    Path::new("/srv/app-data"),
)?;
let metadata = file_system.stat(&LogicalPath::parse("/reports/summary.csv")?)?;
println!("{metadata:?}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

启用 `registry` 后注册 `LocalFileSystemProvider`，再解析经过校验的 `file:` 配置：

```rust
use qubit_fs::ConnectionUri;
use qubit_fs_registry::FileSystemRegistry;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_registry::FileSystemConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = FileSystemRegistry::default();
    registry.register(LocalFileSystemProvider::new())?;

    let config = FileSystemConfig::new(ConnectionUri::parse("file:///tmp/example.txt")?);
    let resolution = registry.resolve_config(&config)?;
    let metadata = resolution
        .file_system()
        .stat(resolution.path())?;
    println!("{metadata:?}");
    Ok(())
}
```

## 路径语义

- 两种门面都使用绝对、层级化的 `qubit_fs::Path`。
- `LocalFileSystems::host()` 映射到进程主机命名空间；`rooted_with_id()` 保留一个
  native root authority，并要求调用方提供稳定的 `FileSystemId`。
- adapter 将 canonical component 转换、rooted containment 与 native 文件操作委托给
  `qubit-local-files`。
- 可选 provider 仅接受无远程 authority、query、options、metadata 或 credentials 的
  绝对 `file:` URI。

## 属性

两种门面均声明 `List`、`Read`、`Write`、`Append`、`CreateDirectory`、
`Delete`、`RecursiveDelete`、`Rename`、`Copy`、临时资源、`AtomicReplace` 与
`AtomicTempPersist`。依赖主机的限制会报告为 `FileSystemLimits::unknown()`。

当前版本仅提供同步 API。若后续增加异步支持，将以可选 feature 发布，不增加默认
依赖面。

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
