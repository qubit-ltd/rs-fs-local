// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::fs;
use std::path::Path;
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
    let fs_dependency = root.join("../rs-fs");
    let registry_dependency = root.join("../rs-fs-registry");
    let filesystem = if fs_dependency.join("Cargo.toml").is_file() {
        format!("{{ path = \"{}\" }}", toml_path(&fs_dependency))
    } else {
        "\"0.2\"".to_owned()
    };
    let registry = if registry_dependency.join("Cargo.toml").is_file() {
        format!("{{ path = \"{}\" }}", toml_path(&registry_dependency))
    } else {
        "\"0.1\"".to_owned()
    };
    let manifest = format!(
        "[package]\nname = \"local-documentation-check\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\nqubit-fs = {filesystem}\nqubit-fs-registry = {registry}\nqubit-fs-local = {{ path = \"{}\", features = [\"registry\"] }}\n",
        toml_path(root),
    );
    fs::write(workspace.path().join("Cargo.toml"), manifest)
        .expect("manifest should be written");

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
            &fs::read_to_string(root.join(document))
                .expect("document should be readable"),
        );
        assert!(!blocks.is_empty(), "{document} must have Rust examples");
        for (block_index, block) in blocks.iter().enumerate() {
            let code = format!(
                "fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{block}\n}}\n"
            );
            fs::write(
                bin.join(format!("doc_{document_index}_{block_index}.rs")),
                code,
            )
            .expect("example source should be written");
        }
    }

    let status = Command::new(env!("CARGO"))
        .args(["check", "--quiet", "--bins"])
        .current_dir(workspace.path())
        .env(
            "CARGO_TARGET_DIR",
            root.join("target/documentation-examples"),
        )
        .status()
        .expect("compile document examples");
    assert!(status.success(), "document examples must compile");
}
