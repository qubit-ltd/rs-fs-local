# qubit-fs-local

[![Rust CI](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-fs-local/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-fs-local/coverage-badge.json)](https://qubit-ltd.github.io/rs-fs-local/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-fs-local.svg?color=blue)](https://crates.io/crates/qubit-fs-local)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

`qubit-fs-local` 为
[`qubit-fs`](https://crates.io/crates/qubit-fs) 提供同步的主机本地 `file:`
文件系统后端。它支持同步读写，并提供以目录 descriptor 为 authority 的
`RootedLocalFileSystem`。通过 `file` provider 别名进行 registry 集成的能力
位于可选的 `registry` feature 中。

## 安装

将本 crate 添加到项目。只有应用通过 `qubit-fs-registry` 使用
`LocalFileSystemProvider` 时才启用 `registry`：

```bash
cargo add qubit-fs-local
cargo add qubit-fs-local --features registry
```

## 使用方法

启用 `registry` 后注册 `LocalFileSystemProvider`，再通过经过校验的 `file:` URI
解析本地资源：

```rust
use qubit_fs::{FsUri, ReadOptions};
use qubit_fs_registry::FileSystemRegistry;
use qubit_fs_local::LocalFileSystemProvider;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = FileSystemRegistry::default();
    registry.register(LocalFileSystemProvider)?;

    let uri = FsUri::parse("file:///tmp/example.txt")?;
    let resource = registry.resource_uri(&uri)?;
    let metadata = resource.stat()?;
    let content = resource.read_all(1024 * 1024)?;
    let _streaming_reader = resource.open_reader(ReadOptions::default())?;
    println!("{metadata:?}");
    println!("读取了 {} 字节", content.len());
    Ok(())
}
```

## 路径语义

- `LocalFileSystem` 拥有主机范围权限，只接受本机绝对路径。
- 规范化的 `FsPath` 文本逐 component 通过 `OsStrPathCodec` 转换，能够保留字面
  `%`、Unix 非 UTF-8 字节和 Windows 原生路径代码单元，同时防止解码后的 separator
  穿越 component 边界。
- `RootedLocalFileSystem` 当前仅支持 Unix。它以一个已打开目录为 descriptor-relative
  authority，调用方必须提供稳定的 filesystem ID，并且同样按 component 转换原生路径。
- Rooted `stat` 可在不跟随最终符号链接的前提下查询根目录、目录、特殊文件和最终
  符号链接本身。
- `stat` 使用 `symlink_metadata`，因此返回最终符号链接本身的信息而不会跟随它。
- `open_reader` 执行阻塞式本地 I/O，并按值接收 `ReadOptions`。当前后端只支持
  顺序整文件读取；range、conditional 和 required-checksum 请求会在预检阶段失败。
- provider 接受无 authority 或空 authority 的 `file:` URI，并拒绝远程
  authority、query、provider options、credentials，以及解码后会引入原生路径边界的
  URI component。

## 属性

`LocalFileSystem` 声明 `Read`、`Write`、`Append` 和 `AtomicReplace`。
`RootedLocalFileSystem` 声明相同能力，并在已打开的 root 下执行原子替换。
`stat` 属于文件系统基础契约，因此不再需要 capability 标记。
依赖主机的路径和 I/O 限制会明确报告为 `FileSystemLimits::unknown()`。

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
