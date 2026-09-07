// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn rust_blocks(markdown: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        if line.trim() == "```rust" {
            assert!(current.is_none(), "nested Rust fence");
            current = Some(String::new());
        } else if line.trim() == "```" {
            if let Some(block) = current.take() {
                result.push(block);
            }
        } else if let Some(block) = current.as_mut() {
            block.push_str(line.strip_prefix("# ").unwrap_or(line));
            block.push('\n');
        }
    }
    assert!(current.is_none(), "unterminated Rust fence");
    result
}

fn toml_path(path: &Path) -> String {
    path.to_str()
        .expect("Cargo manifest path must be UTF-8")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[test]
fn test_readme_and_user_guide_examples_compile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = tempfile::tempdir().expect("isolated example sources");
    let bin = workspace.path().join("src/bin");
    fs::create_dir_all(&bin).expect("bin directory");
    let package: toml::Value = fs::read_to_string(root.join("Cargo.toml"))
        .expect("read manifest")
        .parse()
        .expect("parse manifest");
    let filesystem = dependency_spec(root, &package["dependencies"]["qubit-fs"]);
    let registry = dependency_spec(root, &package["dependencies"]["qubit-fs-registry"]);
    let spi = dependency_spec(root, &package["dependencies"]["qubit-spi"]);
    let manifest = format!(
        "[package]\nname = \"local-documentation-check\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\nqubit-fs = {filesystem}\nqubit-fs-registry = {registry}\nqubit-fs-local = {{ path = \"{}\", features = [\"registry\"] }}\nqubit-spi = {spi}\n",
        toml_path(root),
    );
    fs::write(workspace.path().join("Cargo.toml"), manifest).expect("manifest should be written");

    for (document_index, document) in [
        "README.md",
        "README.zh_CN.md",
        "doc/user_guide.md",
        "doc/user_guide.zh_CN.md",
    ]
    .iter()
    .enumerate()
    {
        let blocks = rust_blocks(
            &fs::read_to_string(root.join(document)).expect("document should be readable"),
        );
        assert!(!blocks.is_empty(), "{document} must have Rust examples");
        for (block_index, block) in blocks.iter().enumerate() {
            let code =
                format!("fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{block}\n}}\n");
            fs::write(
                bin.join(format!("doc_{document_index}_{block_index}.rs")),
                code,
            )
            .expect("example source should be written");
        }
    }

    let metadata = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1"])
        .current_dir(workspace.path())
        .output()
        .expect("resolve example dependencies");
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let graph: serde_json::Value =
        serde_json::from_slice(&metadata.stdout).expect("parse dependency graph");
    for name in [
        "qubit-fs",
        "qubit-fs-registry",
        "qubit-fs-local",
        "qubit-spi",
    ] {
        assert_eq!(
            graph["packages"]
                .as_array()
                .expect("package list")
                .iter()
                .filter(|p| p["name"].as_str() == Some(name))
                .count(),
            1,
            "unique package identity for {name}"
        );
    }
    let status = Command::new(env!("CARGO"))
        .args(["check", "--locked", "--quiet", "--bins"])
        .current_dir(workspace.path())
        .env(
            "CARGO_TARGET_DIR",
            root.join("target/documentation-examples"),
        )
        .status()
        .expect("compile document examples");
    assert!(status.success(), "document examples must compile");
}

/// Renders a dependency from its actual version declaration, retaining an
/// available sibling.
fn dependency_spec(root: &Path, value: &toml::Value) -> String {
    let mut table = value
        .as_table()
        .expect("versioned dependency table")
        .clone();
    assert!(
        table.get("version").and_then(toml::Value::as_str).is_some(),
        "version is required"
    );
    table.remove("optional");
    if let Some(path) = table.remove("path") {
        if !root.join("Cargo.toml").is_file() {
            return toml::Value::Table(table).to_string();
        }
        let declared_path = Path::new(path.as_str().expect("dependency path"));
        let path = resolve_dependency_path(root, declared_path);
        if path.join("Cargo.toml").is_file() {
            let path = path.canonicalize().expect("dependency path must resolve");
            table.insert(
                "path".into(),
                toml::Value::String(path.to_str().expect("UTF-8 path").to_owned()),
            );
        }
    }
    toml::Value::Table(table).to_string()
}

/// Resolves a dependency against the isolated sibling view when Cargo has
/// temporarily rewritten its manifest path to an absolute checkout path.
fn resolve_dependency_path(root: &Path, declared_path: &Path) -> PathBuf {
    let Some(sibling_root) = std::env::var_os("QUBIT_FS_SIBLING_ROOT") else {
        return root.join(declared_path);
    };
    let sibling_root = PathBuf::from(sibling_root);
    let direct = sibling_root.join(
        declared_path
            .file_name()
            .expect("dependency path must name a sibling crate"),
    );
    if direct.join("Cargo.toml").is_file() {
        return direct;
    }
    let package = fs::read_to_string(declared_path.join("Cargo.toml"))
        .ok()
        .and_then(|source| source.parse::<toml::Value>().ok())
        .and_then(|value| value["package"]["name"].as_str().map(str::to_owned));
    if let Some(package) = package
        && let Some(suffix) = package.strip_prefix("qubit-")
    {
        let mapped = sibling_root.join(format!("rs-{suffix}"));
        if mapped.join("Cargo.toml").is_file() {
            return mapped;
        }
    }
    root.join(declared_path)
}

/// Missing siblings retain the current declared requirements, rather than
/// historical constants.
#[test]
fn test_documentation_dependencies_without_siblings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input: toml::Value = fs::read_to_string(root.join("Cargo.toml"))
        .expect("read manifest")
        .parse()
        .expect("parse manifest");
    for name in ["qubit-fs", "qubit-fs-registry", "qubit-spi"] {
        let spec = dependency_spec(
            Path::new("/nonexistent/local-documentation"),
            &input["dependencies"][name],
        );
        let parsed: toml::Value = format!("dependency = {spec}")
            .parse()
            .expect("valid generated dependency");
        assert_eq!(
            parsed["dependency"]["version"],
            input["dependencies"][name]["version"]
        );
        assert!(parsed["dependency"].get("path").is_none());
    }
}

#[test]
fn test_current_documentation_versions_and_signatures_follow_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for document in [
        "README.md",
        "README.zh_CN.md",
        "doc/user_guide.md",
        "doc/user_guide.zh_CN.md",
    ] {
        let text = fs::read_to_string(root.join(document)).expect("document should be readable");
        assert!(
            !text.contains("qubit-fs-local 0.1")
                && !text.contains("qubit-fs-local 0.2")
                && !text.contains("qubit-fs-local 0.3")
                && !text.contains("0.1 release")
                && !text.contains("0.2 release")
                && !text.contains("0.3 release"),
            "{document} must describe the 0.4 API"
        );
    }
    for document in ["README.md", "README.zh_CN.md"] {
        let text = fs::read_to_string(root.join(document)).expect("README should be readable");
        assert!(
            text.contains("rooted_with_id(")
                && text.contains("LocalResourcePolicy"),
            "{document} must explain the three-argument rooted_with_id API"
        );
    }
}
