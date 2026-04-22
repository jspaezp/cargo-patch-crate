// src/manifest.rs
use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub patch_crates: Vec<String>,
}

#[derive(Deserialize, Default)]
struct RawManifest {
    #[serde(default)]
    package: Option<RawPackage>,
    #[serde(default)]
    workspace: Option<RawWorkspace>,
}

#[derive(Deserialize, Default)]
struct RawPackage {
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
struct RawWorkspace {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

pub fn discover(start: &Path) -> Result<Workspace> {
    let root = find_root(start)?;
    let root_toml_path = root.join("Cargo.toml");
    let root_raw = parse_manifest(&root_toml_path)?;

    let mut patch_crates: Vec<String> = Vec::new();

    if let Some(ws) = &root_raw.workspace {
        push_patch_crates(&ws.metadata, &mut patch_crates);
    }
    if let Some(pkg) = &root_raw.package {
        push_patch_crates(&pkg.metadata, &mut patch_crates);
    }

    // Walk workspace members if this is a workspace.
    if let Some(ws) = root_raw.workspace {
        for member_pattern in &ws.members {
            for member_dir in expand_member(&root, member_pattern)? {
                let member_toml = member_dir.join("Cargo.toml");
                if !member_toml.is_file() {
                    continue;
                }
                let member_raw = parse_manifest(&member_toml)?;
                if let Some(pkg) = member_raw.package {
                    push_patch_crates(&pkg.metadata, &mut patch_crates);
                }
            }
        }
    }

    Ok(Workspace { root, patch_crates })
}

fn find_root(start: &Path) -> Result<PathBuf> {
    // First, walk up to the nearest Cargo.toml (package or workspace).
    let mut nearest: Option<PathBuf> = None;
    for dir in start.ancestors() {
        if dir.join("Cargo.toml").is_file() {
            nearest = Some(dir.to_path_buf());
            break;
        }
    }
    let nearest = nearest
        .ok_or_else(|| anyhow!("no Cargo.toml found walking up from {:?}", start))?;

    // Then, keep walking up looking for a [workspace] root that contains nearest.
    for dir in nearest.ancestors() {
        let toml = dir.join("Cargo.toml");
        if !toml.is_file() {
            continue;
        }
        let raw = parse_manifest(&toml)?;
        if raw.workspace.is_some() {
            return Ok(dir.to_path_buf());
        }
    }
    Ok(nearest)
}

fn parse_manifest(path: &Path) -> Result<RawManifest> {
    let text = fs::read_to_string(path)
        .map_err(|e| anyhow!("failed to read {:?}: {}", path, e))?;
    toml::from_str(&text).map_err(|e| anyhow!("failed to parse {:?}: {}", path, e))
}

fn push_patch_crates(metadata: &Option<serde_json::Value>, out: &mut Vec<String>) {
    let Some(md) = metadata else { return };
    let Some(crates) = md
        .get("patch")
        .and_then(|p| p.get("crates"))
        .and_then(|c| c.as_array())
    else {
        return;
    };
    for c in crates {
        if let Some(n) = c.as_str() {
            out.push(n.to_string());
        }
    }
}

fn expand_member(root: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    if !pattern.contains('*') {
        return Ok(vec![root.join(pattern)]);
    }
    // Only support a trailing "*" segment ("crates/*"). Reject other globs.
    let (prefix, last) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    if last != "*" || prefix.contains('*') {
        bail!("unsupported workspace member glob: {}", pattern);
    }
    let base = root.join(prefix);
    let mut out = Vec::new();
    if base.is_dir() {
        for entry in fs::read_dir(&base)? {
            let p = entry?.path();
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn single_package_collects_package_metadata() {
        let dir = tmp();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[package]
name = "x"
version = "0.1.0"
edition = "2021"

[package.metadata.patch]
crates = ["home"]
"#,
        )
        .unwrap();

        let ws = discover(dir.path()).unwrap();
        assert_eq!(ws.root, dir.path());
        assert_eq!(ws.patch_crates, vec!["home".to_string()]);
    }

    #[test]
    fn workspace_collects_workspace_metadata() {
        let dir = tmp();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = []

[workspace.metadata.patch]
crates = ["home", "anyhow"]
"#,
        )
        .unwrap();

        let ws = discover(dir.path()).unwrap();
        assert_eq!(ws.root, dir.path());
        assert_eq!(
            ws.patch_crates,
            vec!["home".to_string(), "anyhow".to_string()]
        );
    }

    #[test]
    fn workspace_member_glob_merges_package_metadata() {
        let dir = tmp();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["crates/*"]
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("crates/a")).unwrap();
        fs::write(
            dir.path().join("crates/a/Cargo.toml"),
            r#"
[package]
name = "a"
version = "0.1.0"
edition = "2021"

[package.metadata.patch]
crates = ["home"]
"#,
        )
        .unwrap();

        let ws = discover(&dir.path().join("crates/a")).unwrap();
        assert_eq!(ws.root, dir.path());
        assert_eq!(ws.patch_crates, vec!["home".to_string()]);
    }
}
