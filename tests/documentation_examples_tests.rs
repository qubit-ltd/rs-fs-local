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
use std::path::absolute;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExampleProfile {
    Direct,
    Registry,
    Selection,
}

impl ExampleProfile {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "direct" => Some(Self::Direct),
            "registry" => Some(Self::Registry),
            "selection" => Some(Self::Selection),
            _ => None,
        }
    }
}

fn example_profiles(markdown: &str) -> Result<Vec<ExampleProfile>, String> {
    let mut pending = None;
    let mut result = Vec::new();
    for line in markdown.lines() {
        if let Some(value) = line
            .strip_prefix("<!-- fs-local-example: ")
            .and_then(|line| line.strip_suffix(" -->"))
        {
            pending = Some(value.to_owned());
        } else if line.trim() == "```rust" {
            let value = pending
                .take()
                .ok_or_else(|| "Rust fence has no profile marker".to_owned())?;
            result.push(ExampleProfile::parse(&value).ok_or_else(|| format!("unknown profile {value}"))?);
        } else if !line.trim().is_empty() {
            pending = None;
        }
    }
    Ok(result)
}

fn profile_dependencies(profile: ExampleProfile) -> Vec<&'static str> {
    match profile {
        ExampleProfile::Direct => vec!["qubit-fs", "qubit-fs-local"],
        ExampleProfile::Registry => vec!["qubit-fs", "qubit-fs-local", "qubit-fs-registry"],
        ExampleProfile::Selection => vec!["qubit-fs", "qubit-fs-local", "qubit-fs-registry", "qubit-spi"],
    }
}

fn manifest_dependency_names(manifest: &toml::Value) -> Vec<String> {
    let mut names = manifest["dependencies"]
        .as_table()
        .expect("generated dependencies table")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn dependency_requirement(manifest: &str, name: &str) -> String {
    let parsed: toml::Value = toml::from_str(manifest).unwrap();
    parsed["dependencies"][name]["version"].as_str().unwrap().to_owned()
}

fn validate_selection_install(command: &str, requirement: &str) -> Result<(), String> {
    let expected = format!("cargo add qubit-spi@{}", requirement.trim_start_matches('^'));
    if command.trim() == expected {
        Ok(())
    } else {
        Err(format!("expected {expected}"))
    }
}

fn profiled_rust_blocks(markdown: &str) -> Result<Vec<(ExampleProfile, String)>, String> {
    let mut result = Vec::new();
    let mut pending = None;
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        if let Some(value) = line
            .strip_prefix("<!-- fs-local-example: ")
            .and_then(|line| line.strip_suffix(" -->"))
        {
            pending = Some(ExampleProfile::parse(value).ok_or_else(|| format!("unknown profile {value}"))?);
        } else if line.trim() == "```rust" {
            let profile = pending
                .take()
                .ok_or_else(|| "Rust fence has no profile marker".to_owned())?;
            current = Some(String::new());
            pending = Some(profile);
        } else if line.trim() == "```" {
            if let Some(block) = current.take() {
                result.push((
                    pending.take().ok_or_else(|| "Rust fence lost profile".to_owned())?,
                    block,
                ));
            }
        } else if let Some(block) = current.as_mut() {
            block.push_str(line.strip_prefix("# ").unwrap_or(line));
            block.push('\n');
        } else if !line.trim().is_empty() {
            pending = None;
        }
    }
    if current.is_some() {
        return Err("unterminated Rust fence".to_owned());
    }
    Ok(result)
}

fn marked_design_blocks(markdown: &str) -> Vec<(ExampleProfile, String)> {
    let mut result = Vec::new();
    let mut pending = None;
    let mut current: Option<(Option<ExampleProfile>, String)> = None;
    for line in markdown.lines() {
        if let Some(value) = line
            .strip_prefix("<!-- fs-local-example: ")
            .and_then(|line| line.strip_suffix(" -->"))
        {
            pending = Some(ExampleProfile::parse(value).unwrap_or_else(|| panic!("unknown profile {value}")));
        } else if line.trim() == "```rust" {
            current = Some((pending.take(), String::new()));
        } else if line.trim() == "```" {
            if let Some((Some(profile), block)) = current.take() {
                result.push((profile, block));
            } else {
                current = None;
            }
        } else if let Some((_, block)) = current.as_mut() {
            block.push_str(line.strip_prefix("# ").unwrap_or(line));
            block.push('\n');
        } else if !line.trim().is_empty() {
            pending = None;
        }
    }
    result
}

#[test]
fn test_example_profiles_require_known_adjacent_markers() {
    assert!(example_profiles("<!-- fs-local-example: direct -->\n```rust\nlet _ = 1;\n```").is_ok());
    assert!(example_profiles("```rust\nlet _ = 1;\n```").is_err());
    assert!(example_profiles("<!-- fs-local-example: mystery -->\n```rust\nlet _ = 1;\n```").is_err());
}

#[test]
fn test_all_published_rust_examples_have_supported_profiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for document in [
        "README.md",
        "README.zh_CN.md",
        "doc/user_guide.md",
        "doc/user_guide.zh_CN.md",
    ] {
        let source = fs::read_to_string(root.join(document)).unwrap();
        assert!(example_profiles(&source).is_ok(), "{document}");
    }
    for document in [
        "doc/local_file_system_adapter_design.md",
        "doc/local_file_system_adapter_design.zh_CN.md",
    ] {
        let source = fs::read_to_string(root.join(document)).unwrap();
        assert_eq!(
            source.matches("<!-- fs-local-example: direct -->").count(),
            1,
            "{document}"
        );
        assert!(!source.contains("<!-- fs-local-example: registry -->"), "{document}");
        assert!(!source.contains("<!-- fs-local-example: selection -->"), "{document}");
    }
}

#[test]
fn test_direct_profile_does_not_inject_registry_or_spi() {
    let deps = profile_dependencies(ExampleProfile::Direct);
    assert_eq!(deps, ["qubit-fs", "qubit-fs-local"]);
}

#[test]
fn test_selection_profile_requires_spi_install_command() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let requirement = dependency_requirement(&manifest, "qubit-spi");
    assert!(validate_selection_install("cargo add qubit-spi@0.13", &requirement).is_ok());
    assert!(validate_selection_install("cargo add qubit-fs-local@0.11", &requirement).is_err());
    for document in ["doc/user_guide.md", "doc/user_guide.zh_CN.md"] {
        let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(document)).unwrap();
        let marker = source.rfind("<!-- fs-local-example: selection -->").unwrap();
        let preceding = &source[..marker];
        let command = preceding
            .lines()
            .rev()
            .find(|line| line.trim().starts_with("cargo add qubit-spi@"));
        assert!(
            command.is_some_and(|line| validate_selection_install(line, &requirement).is_ok()),
            "{document}"
        );
    }
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
    let source = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
    let package: toml::Value = toml::from_str(&source).expect("parse manifest");
    let filesystem = dependency_spec(root, &package["dependencies"]["qubit-fs"]);
    let registry = dependency_spec(root, &package["dependencies"]["qubit-fs-registry"]);
    let spi = dependency_spec(root, &package["dependencies"]["qubit-spi"]);
    let sibling_root = root.parent().expect("candidate checkout has a parent");
    let mut patches = Vec::new();
    for (package, sibling) in [
        ("qubit-fs", "rs-fs"),
        ("qubit-local-files", "rs-local-files"),
        ("qubit-fs-registry", "rs-fs-registry"),
        ("qubit-redact", "rs-redact"),
        ("qubit-io", "rs-io"),
        ("qubit-spi", "rs-spi"),
    ] {
        let path = sibling_root.join(sibling);
        if path.join("Cargo.toml").is_file() {
            patches.push(format!("{package} = {{ path = \"{}\" }}", toml_path(&path)));
        }
    }
    let documents = [
        "README.md",
        "README.zh_CN.md",
        "doc/user_guide.md",
        "doc/user_guide.zh_CN.md",
        "doc/local_file_system_adapter_design.md",
        "doc/local_file_system_adapter_design.zh_CN.md",
    ];
    let mut examples: Vec<(usize, usize, &str, ExampleProfile, String)> = Vec::new();
    for (document_index, document) in documents.iter().enumerate() {
        let source = fs::read_to_string(root.join(document)).expect("document should be readable");
        let blocks = if document.starts_with("doc/local_file_system_adapter_design") {
            // Design documents contain API sketches; compile only explicitly marked complete examples.
            marked_design_blocks(&source)
        } else {
            profiled_rust_blocks(&source).expect("published Rust fences require a supported adjacent profile")
        };
        assert!(!blocks.is_empty(), "{document} must have Rust examples");
        for (block_index, (profile, block)) in blocks.into_iter().enumerate() {
            examples.push((document_index, block_index, document, profile, block));
        }
    }
    for profile in [
        ExampleProfile::Direct,
        ExampleProfile::Registry,
        ExampleProfile::Selection,
    ] {
        let workspace = tempfile::tempdir().expect("isolated profile example crate");
        let bin = workspace.path().join("src/bin");
        fs::create_dir_all(&bin).expect("bin directory");
        for (document_index, block_index, _, example_profile, block) in &examples {
            if *example_profile != profile {
                continue;
            }
            let code = format!("fn main() -> Result<(), Box<dyn std::error::Error>> {{\n{block}\n}}\n");
            fs::write(bin.join(format!("doc_{document_index}_{block_index}.rs")), code)
                .expect("example source should be written");
        }
        let mut manifest = format!(
            "[package]\nname = \"local-documentation-{}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\nqubit-fs = {filesystem}\nqubit-fs-local = {{ version = \"{}\", path = \"{}\"{} }}\n",
            match profile {
                ExampleProfile::Direct => "direct",
                ExampleProfile::Registry => "registry",
                ExampleProfile::Selection => "selection",
            },
            package["package"]["version"].as_str().expect("local version"),
            toml_path(root),
            if profile == ExampleProfile::Direct {
                String::new()
            } else {
                ", features = [\"registry\"]".to_owned()
            },
        );
        if profile != ExampleProfile::Direct {
            manifest.push_str(&format!("qubit-fs-registry = {registry}\n"));
        }
        if profile == ExampleProfile::Selection {
            manifest.push_str(&format!("qubit-spi = {spi}\n"));
        }
        if !patches.is_empty() {
            manifest.push_str("\n[patch.crates-io]\n");
            manifest.push_str(&patches.join("\n"));
            manifest.push('\n');
        }
        fs::write(workspace.path().join("Cargo.toml"), &manifest).expect("manifest should be written");
        let expected = profile_dependencies(profile);
        let parsed_manifest: toml::Value = toml::from_str(&manifest).expect("generated manifest is valid TOML");
        let mut expected_dependencies = expected.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        expected_dependencies.sort();
        assert_eq!(
            manifest_dependency_names(&parsed_manifest),
            expected_dependencies,
            "direct dependencies for {profile:?}"
        );

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
        let graph: serde_json::Value = serde_json::from_slice(&metadata.stdout).expect("parse dependency graph");
        let mut expected_graph = expected.clone();
        expected_graph.extend(["qubit-fs-local", "qubit-local-files", "qubit-io"]);
        if profile != ExampleProfile::Direct {
            expected_graph.extend(["qubit-fs-registry", "qubit-spi"]);
        }
        expected_graph.sort_unstable();
        expected_graph.dedup();
        for name in expected_graph {
            assert_eq!(
                graph["packages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|p| p["name"].as_str() == Some(name))
                    .count(),
                1,
                "unique package identity for {name} in {profile:?}"
            );
        }
        let resolve = &graph["resolve"]["nodes"];
        let package_id = graph["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| {
                package["name"]
                    == format!(
                        "local-documentation-{}",
                        match profile {
                            ExampleProfile::Direct => "direct",
                            ExampleProfile::Registry => "registry",
                            ExampleProfile::Selection => "selection",
                        }
                    )
            })
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let root_node = resolve
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"].as_str() == Some(package_id))
            .unwrap();
        let mut resolved_direct_dependencies = root_node["deps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|dependency| dependency["name"].as_str().unwrap().replace('_', "-"))
            .collect::<Vec<_>>();
        resolved_direct_dependencies.sort();
        assert_eq!(
            resolved_direct_dependencies, expected_dependencies,
            "resolved direct dependencies for {profile:?}"
        );
        if profile == ExampleProfile::Direct {
            for forbidden in ["qubit-fs-registry", "qubit-spi"] {
                assert!(
                    !graph["packages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|package| package["name"] == forbidden),
                    "{forbidden} package must be absent from direct metadata graph"
                );
                assert!(
                    !resolved_direct_dependencies.iter().any(|name| name == forbidden),
                    "{forbidden} must not be a direct dependency in direct graph"
                );
            }
        }
        let local_id = graph["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "qubit-fs-local")
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let local_node = resolve
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"].as_str() == Some(local_id))
            .unwrap();
        let local_features = local_node["features"].as_array().unwrap();
        assert_eq!(
            local_features
                .iter()
                .any(|feature| feature.as_str() == Some("registry")),
            profile != ExampleProfile::Direct,
            "local feature isolation for {profile:?}"
        );
        let status = Command::new(env!("CARGO"))
            .args(["check", "--locked", "--quiet", "--bins"])
            .current_dir(workspace.path())
            .env("CARGO_TARGET_DIR", root.join("target/documentation-examples"))
            .status()
            .expect("compile document examples");
        assert!(status.success(), "{profile:?} document examples must compile");
        if profile == ExampleProfile::Direct {
            for (document_index, _, document, _, _) in
                examples.iter().filter(|(_, block_index, document, profile, _)| {
                    *block_index == 0
                        && [
                            "README.md",
                            "README.zh_CN.md",
                            "doc/user_guide.md",
                            "doc/user_guide.zh_CN.md",
                        ]
                        .contains(document)
                        && *profile == ExampleProfile::Direct
                })
            {
                let binary = format!("doc_{document_index}_0");
                let status = Command::new(env!("CARGO"))
                    .args(["run", "--locked", "--quiet", "--bin", &binary])
                    .current_dir(workspace.path())
                    .env("CARGO_TARGET_DIR", root.join("target/documentation-examples"))
                    .status()
                    .expect("run first document example");
                assert!(status.success(), "{document} first example must run");
            }
        }
    }
}

/// Renders a dependency from its declaration or sibling package version while
/// retaining the sibling's path identity when available.
fn dependency_spec(root: &Path, value: &toml::Value) -> String {
    let mut table = value.as_table().expect("versioned dependency table").clone();
    if table.get("version").and_then(toml::Value::as_str).is_none() {
        let declared_path = table
            .get("path")
            .and_then(toml::Value::as_str)
            .expect("dependency requires a path or version");
        let sibling = resolve_dependency_path(root, Path::new(declared_path));
        let source = fs::read_to_string(sibling.join("Cargo.toml"))
            .expect("path-only dependency requires an available sibling manifest");
        let manifest: toml::Value = toml::from_str(&source).expect("parse sibling manifest");
        let version = manifest["package"]["version"]
            .as_str()
            .expect("sibling package version");
        table.insert("version".into(), toml::Value::String(version.to_owned()));
    }
    table.remove("optional");
    if let Some(path) = table.remove("path") {
        if !root.join("Cargo.toml").is_file() {
            return toml::Value::Table(table).to_string();
        }
        let declared_path = Path::new(path.as_str().expect("dependency path"));
        let path = resolve_dependency_path(root, declared_path);
        if path.join("Cargo.toml").is_file() {
            let path = absolute(path).expect("dependency path must be absolute");
            table.insert(
                "path".into(),
                toml::Value::String(path.to_str().expect("UTF-8 path").to_owned()),
            );
        }
    }
    toml::Value::Table(table).to_string()
}

/// Path-only development dependencies inherit the sibling package release.
#[test]
fn test_dependency_spec_resolves_path_only_sibling_version() {
    let workspace = tempfile::tempdir().expect("isolated path dependency fixture");
    let root = workspace.path().join("consumer");
    let sibling = workspace.path().join("path-only-fixture");
    fs::create_dir_all(&root).expect("consumer directory");
    fs::create_dir_all(&sibling).expect("sibling directory");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'consumer'\nversion = '0.1.0'\n",
    )
    .expect("consumer manifest");
    fs::write(
        sibling.join("Cargo.toml"),
        "[package]\nname = 'qubit-fs'\nversion = '0.2.0'\n",
    )
    .expect("sibling manifest");
    let input: toml::Value =
        toml::from_str("dependency = { path = '../path-only-fixture' }").expect("path-only dependency");
    let spec = dependency_spec(&root, &input["dependency"]);
    let rendered: toml::Value = toml::from_str(&format!("dependency = {spec}")).expect("parse dependency");

    assert_eq!(rendered["dependency"]["version"].as_str(), Some("0.2.0"));
    let resolved = Path::new(rendered["dependency"]["path"].as_str().expect("resolved path"));
    assert!(resolved.join("Cargo.toml").is_file());
}

/// Cargo must see the same symlink spelling in direct and transitive
/// dependencies.
#[cfg(unix)]
#[test]
fn test_dependency_spec_preserves_sibling_symlink_identity() {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir().expect("isolated sibling fixture");
    let root = workspace.path().join("consumer");
    let target = workspace.path().join("source");
    let alias = root.join("linked-dependency");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'consumer'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(
        target.join("Cargo.toml"),
        "[package]\nname = 'dependency'\nversion = '0.1.0'\n",
    )
    .unwrap();
    symlink(&target, &alias).unwrap();
    let input: toml::Value = toml::from_str("dependency = { version = '0.1', path = 'linked-dependency' }").unwrap();
    let spec = dependency_spec(&root, &input["dependency"]);
    let output: toml::Value = toml::from_str(&format!("dependency = {spec}")).unwrap();
    assert_eq!(output["dependency"]["path"].as_str(), alias.to_str());
    assert_ne!(alias, alias.canonicalize().unwrap());
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
        .and_then(|source| toml::from_str::<toml::Value>(&source).ok())
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

/// A versioned dependency remains usable when its checkout is unavailable.
#[test]
fn test_versioned_documentation_dependency_without_sibling() {
    let input: toml::Value =
        toml::from_str("dependency = { version = '0.7', path = '../missing' }").expect("synthetic dependency");
    let spec = dependency_spec(Path::new("/nonexistent/local-documentation"), &input["dependency"]);
    let parsed: toml::Value = toml::from_str(&format!("dependency = {spec}")).expect("valid generated dependency");

    assert_eq!(parsed["dependency"]["version"].as_str(), Some("0.7"));
    assert!(parsed["dependency"].get("path").is_none());
}

/// A path-only dependency must not invent a publishable requirement.
#[test]
#[should_panic(expected = "path-only dependency requires an available sibling manifest")]
fn test_path_only_documentation_dependency_without_sibling_is_rejected() {
    let input: toml::Value = toml::from_str("dependency = { path = '../missing' }").expect("synthetic dependency");
    let _ = dependency_spec(Path::new("/nonexistent/local-documentation"), &input["dependency"]);
}

#[test]
fn test_current_documentation_versions_and_signatures_follow_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
    let manifest: toml::Value = toml::from_str(&source).expect("parse manifest");
    let version = manifest["package"]["version"]
        .as_str()
        .expect("package version")
        .to_owned();
    let mut version_parts = version.split('.');
    let major = version_parts.next().expect("package major version");
    let minor = version_parts.next().expect("package minor version");
    let version_marker = format!("`{major}.{minor}");
    for document in [
        "README.md",
        "README.zh_CN.md",
        "doc/user_guide.md",
        "doc/user_guide.zh_CN.md",
    ] {
        let text = fs::read_to_string(root.join(document)).expect("document should be readable");
        assert!(
            text.contains(&version_marker),
            "{document} must identify the manifest's current major/minor version"
        );
        let stale_local_versions = [
            "qubit-fs-local@0.1.",
            "qubit-fs-local@0.2",
            "qubit-fs-local@0.3",
            "qubit-fs-local 0.1.",
            "qubit-fs-local 0.2",
            "qubit-fs-local 0.3",
            "qubit-fs-local = \"0.1",
            "qubit-fs-local = \"0.2",
            "qubit-fs-local = \"0.3",
            "`qubit-fs-local` = \"0.1",
            "`qubit-fs-local` = \"0.2",
            "`qubit-fs-local` = \"0.3",
            "`qubit-fs-local` 0.1.",
            "`qubit-fs-local` 0.2",
            "`qubit-fs-local` 0.3",
        ];
        assert!(
            stale_local_versions.iter().all(|token| !text.contains(token)),
            "{document} must not describe an obsolete API version"
        );
    }
    for document in ["README.md", "README.zh_CN.md"] {
        let text = fs::read_to_string(root.join(document)).expect("README should be readable");
        assert!(
            text.contains("rooted_with_id(")
                && text.contains("LocalResourcePolicy")
                && text.contains("LocalResourcePolicy::standard()")
                && text.contains("bounded(list, copy, delete)")
                && text.contains("rooted_with_id(id, root, policy)"),
            "{document} must explain the three-argument rooted_with_id API"
        );
        assert!(
            !text.contains("rooted_with_id(id, root)`"),
            "{document} must not retain the old two-argument rooted_with_id signature"
        );
    }
    let native_dependency = &manifest["dependencies"]["qubit-local-files"];
    let native_version = native_dependency
        .as_str()
        .or_else(|| native_dependency.get("version").and_then(toml::Value::as_str))
        .expect("native dependency version");
    let marker = format!("`qubit-local-files` {native_version}");
    for document in [
        "README.md",
        "README.zh_CN.md",
        "doc/user_guide.md",
        "doc/user_guide.zh_CN.md",
        "doc/local_file_system_adapter_design.md",
        "doc/local_file_system_adapter_design.zh_CN.md",
    ] {
        let text = fs::read_to_string(root.join(document)).expect("document should be readable");
        assert!(text.contains(&marker), "{document} must name current native version");
        assert!(
            !text.contains("core 0.8") && !text.contains("核心 0.8") && !text.contains("Version 0.8"),
            "{document} retains obsolete version"
        );
    }
}
