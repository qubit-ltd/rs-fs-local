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

use qubit_fs::{FileSystemId, Path as LogicalPath};
use qubit_fs_local::{LocalFileSystems, LocalResourcePolicy};

let file_system = LocalFileSystems::rooted_with_id(
    FileSystemId::new("app-data")?,
    Path::new("/srv/app-data"),
    LocalResourcePolicy::unbounded(),
)?;
let metadata = file_system.stat(&LogicalPath::parse("/reports/summary.csv")?)?;
println!("{metadata:?}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

所有构造函数都要求显式传入 `LocalResourcePolicy`：只有应用明确接受无界递归工作时才使用
`unbounded()`，否则使用带完整 list/copy 预算的 `bounded(...)`。`LocalFileSystems::host(policy)`
打开进程主机命名空间；`rooted(root, policy)` 生成进程本地标识；`rooted_with_id(id, root, policy)`
保留调用方提供的标识。若该标识必须在进程之外保持稳定，应使用后者。

## 提供的能力

- 通过 host 和 rooted 构造路径提供本地文件的具体同步 `FileSystem` 门面。
- 可选的 `LocalFileSystemProvider` 位于 `registry` feature 后；它将支持的 `file:` 配置
  解析为文件系统、路径和 canonical URI。
- provider 仅接受绝对 `file:` URI；会拒绝远程 authority、query、相对路径、非 `file`
  scheme，以及配置 options 或 credentials。
- 如果同一个 registry 需要注册多个 rooted authority，请使用
  `LocalFileSystemProvider::rooted_with_descriptor`，每个 descriptor ID 会成为其文件系统的
  provider identity。

当前版本仅提供同步本地文件系统门面，不提供异步本地文件系统门面。

## 延伸阅读

- [English user guide](doc/user_guide.md)
- [中文用户手册](doc/user_guide.zh_CN.md)
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
