//!
//! patch-crate lets rust app developer instantly make and keep fixes to crate dependencies.
//! It's a vital band-aid for those of us living on the bleeding edge.
//!
//! # Installation
//!
//! Simply run:
//! ```sh
//! cargo install patch-crate
//! ```
//!
//! # Usage
//!
//! To patch dependency one has to add the following
//! to `Cargo.toml`
//!
//! ```toml
//! [package.metadata.patch]
//! crates = ["serde"]
//! ```
//!
//! It specifies which dependency to patch (in this case
//! serde). Running:
//!
//! ```sh
//! cargo patch-crate
//! ```
//!
//! will download the sede package specified in the
//! dpendency section to the `target/patch` folder.
//!
//! Then override the dependency using
//! `replace` like this
//!
//! ```toml
//! [patch.crates-io]
//! serde = { path = './target/patch/serde-1.0.110' }
//! ```
//!
//! fix a bug in './target/patch/serde-1.0.110' directly.
//!
//! run following to create a `patches/serde+1.0.110.patch` file
//! ```sh
//! cargo patch-crate serde
//! ```
//!
//! commit the patch file to share the fix with your team
//! ```sh
//! git add patches/serde+1.0.110.patch
//! git commit -m "fix broken-serde in serde"
//! ```

use anyhow::{Ok, Result, anyhow};
use cargo_metadata::{Metadata, MetadataCommand, Package, PackageId};
use clap::Parser;
use fs_extra::dir::{CopyOptions, copy};
use log::*;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

const PATCH_EXT: &str = "patch";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    crates: Vec<String>,
    #[arg(short, long)]
    force: bool,
}

fn patches_folder(md: &Metadata) -> PathBuf {
    md.workspace_root.as_std_path().join("patches")
}

fn patch_target_folder(md: &Metadata) -> PathBuf {
    md.workspace_root.as_std_path().join("target/patch")
}

fn patch_target_tmp_folder(md: &Metadata) -> PathBuf {
    md.workspace_root.as_std_path().join("target/patch-tmp")
}

fn clean_patch_folder(md: &Metadata) -> Result<()> {
    let p = patch_target_folder(md);
    if p.exists() {
        fs::remove_dir_all(p)?;
    }
    Ok(())
}

fn pkg_root(pkg: &Package) -> &Path {
    pkg.manifest_path
        .parent()
        .expect("manifest_path has parent")
        .as_std_path()
}

fn pkg_slug(pkg: &Package) -> Result<&str> {
    pkg_root(pkg)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Dependency Folder does not have a name"))
}

fn patch_target_path(pkg: &Package, md: &Metadata) -> Result<PathBuf> {
    Ok(patch_target_folder(md).join(pkg_slug(pkg)?))
}

fn query<'a>(md: &'a Metadata, q: &str) -> Result<&'a Package> {
    let (name, ver) = match q.split_once('@') {
        Some((n, v)) => (n, Some(v)),
        None => (q, None),
    };
    let matches: Vec<&Package> = md
        .packages
        .iter()
        .filter(|p| p.name == name && ver.is_none_or(|v| p.version.to_string() == v))
        .collect();
    match matches.len() {
        0 => Err(anyhow!("package `{}` not found", q)),
        1 => Ok(matches[0]),
        _ => Err(anyhow!(
            "multiple versions of `{}` found; specify with name@version",
            name
        )),
    }
}

fn copy_package(pkg: &Package, dst_folder: &Path, overwrite: bool) -> Result<PathBuf> {
    fs::create_dir_all(dst_folder)?;
    let dst = dst_folder.join(pkg_slug(pkg)?);
    if dst.exists() {
        if overwrite {
            info!("crate: {}, copy to {:?}", pkg.name, dst_folder);
            fs::remove_dir_all(&dst)?;
        } else {
            info!("crate: {}, skip, {:?} already exists.", pkg.name, &dst);
            return Ok(dst);
        }
    }
    copy(pkg_root(pkg), dst_folder, &CopyOptions::new())?;
    Ok(dst)
}

fn load_metadata() -> Result<Metadata> {
    MetadataCommand::new()
        .exec()
        .map_err(|e| anyhow!("cargo metadata failed: {}", e))
}

fn collect_patch_list(md: &Metadata) -> Result<HashSet<PackageId>> {
    let mut list: HashSet<PackageId> = HashSet::new();
    let mut values: Vec<&serde_json::Value> = Vec::new();
    if !md.workspace_metadata.is_null() {
        values.push(&md.workspace_metadata);
    }
    for pid in &md.workspace_members {
        if let Some(p) = md.packages.iter().find(|p| &p.id == pid)
            && !p.metadata.is_null()
        {
            values.push(&p.metadata);
        }
    }
    for v in values {
        let Some(crates) = v
            .get("patch")
            .and_then(|p| p.get("crates"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for c in crates {
            if let Some(n) = c.as_str() {
                list.insert(query(md, n)?.id.clone());
            }
        }
    }
    Ok(list)
}

pub fn run() -> anyhow::Result<()> {
    let args = {
        let mut args = Cli::parse();
        if let Some(idx) = args.crates.iter().position(|c| c == "patch-crate") {
            args.crates.remove(idx);
        }
        args
    };

    let md = load_metadata()?;

    let patches_folder = patches_folder(&md);
    let patch_target_folder = patch_target_folder(&md);
    let patch_target_tmp_folder = patch_target_tmp_folder(&md);

    if !args.crates.is_empty() {
        info!("starting patch creation.");
        if !patches_folder.exists() {
            fs::create_dir_all(&patches_folder)?;
        }
        for n in args.crates.iter() {
            info!("crate: {}, starting patch creation.", n);
            let pkg = query(&md, n)?;
            let patched_crate_path = patch_target_path(pkg, &md)?;

            let original_crate_path = copy_package(pkg, &patch_target_tmp_folder, true)?;
            git::init(&original_crate_path)?;

            let original_crate_git_path = original_crate_path.join(".git");
            let patched_crate_git_path = patched_crate_path.join(".git");

            git::destroy(&patched_crate_path)?;
            copy(
                &original_crate_git_path,
                &patched_crate_git_path,
                &CopyOptions::new().overwrite(true).copy_inside(true),
            )?;

            let patch_file =
                patches_folder.join(format!("{}+{}.{}", pkg.name, pkg.version, PATCH_EXT));
            git::create_patch(&patched_crate_path, &patch_file)?;
            fs::remove_dir_all(&patch_target_tmp_folder)?;

            git::destroy(&patched_crate_path)?;
            info!("crate: {}, create patch successfully, {:?}", n, &patch_file);
        }
    } else {
        info!("applying patch");

        let mut crates_to_patch = collect_patch_list(&md)?;

        if args.force {
            info!("Cleaning up patch folder.");
            clean_patch_folder(&md)?;
        }

        if patches_folder.exists() {
            for entry in fs::read_dir(&patches_folder)? {
                let entry = entry?;
                if entry.metadata()?.is_file()
                    && entry.path().extension() == Some(OsStr::new(PATCH_EXT))
                {
                    let patch_file = entry.path();
                    let filename = patch_file
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or(anyhow!("Patch file does not have a name"))?;

                    if let Some((pkg_name, version)) = filename.split_once('+') {
                        let pkg = query(&md, &format!("{}@{}", pkg_name, version))?;
                        if !crates_to_patch.contains(&pkg.id) {
                            warn!(
                                "crate: {}, {} is not in the [package.metadata.patch] or [workspace.metadata.patch] section of Cargo.toml. Did you forget to add it?",
                                pkg_name, pkg_name
                            );
                            continue;
                        }

                        let target = patch_target_path(pkg, &md)?;
                        if !target.exists() {
                            copy_package(pkg, &patch_target_folder, args.force)?;
                            info!("crate: {}, applying patch started.", pkg_name);
                            git::init(&target)?;
                            git::apply(&target, &patch_file)?;
                            git::destroy(&target)?;
                            info!(
                                "crate: {}, successfully applied patch {:?}.",
                                pkg_name, patch_file
                            );
                        } else {
                            info!(
                                "crate: {}, skip applying patch, {:?} already exists. Did you forget to add `--force`?",
                                pkg_name, target
                            );
                        }
                        crates_to_patch.remove(&pkg.id);
                    }
                }
            }
        }
        for pid in crates_to_patch {
            if let Some(pkg) = md.packages.iter().find(|p| p.id == pid) {
                copy_package(pkg, &patch_target_folder, args.force)?;
            }
        }
    }

    info!("Done");
    Ok(())
}

mod log {
    pub use paris::*;
}

mod git {
    use std::{ffi::OsStr, fs, path::Path, process::Command};

    pub fn init(repo_dir: &Path) -> anyhow::Result<()> {
        Command::new("git")
            .current_dir(repo_dir)
            .args(["init"])
            .output()?;
        Command::new("git")
            .current_dir(repo_dir)
            .args(["add", "."])
            .output()?;
        Command::new("git")
            .current_dir(repo_dir)
            .args(["commit", "-m", "zero"])
            .output()?;
        Ok(())
    }

    pub fn apply(repo_dir: &Path, patch_file: &Path) -> anyhow::Result<()> {
        #[cfg(target_os = "windows")]
        let patch_file = patch_file
            .to_string_lossy()
            .to_string()
            .trim_start_matches(r#"\\?\"#)
            .to_string();
        #[cfg(not(target_os = "windows"))]
        let patch_file = patch_file.to_string_lossy().to_string();

        let out = Command::new("git")
            .current_dir(repo_dir)
            .args([
                "apply",
                "--ignore-space-change",
                "--ignore-whitespace",
                "--whitespace=nowarn",
                &patch_file,
            ])
            .output()?;

        if !out.status.success() {
            anyhow::bail!(String::from_utf8(out.stderr)?)
        }
        Ok(())
    }
    pub fn destroy(repo_dir: &Path) -> anyhow::Result<()> {
        let git_dir = repo_dir.join(".git");
        if git_dir.exists() {
            fs::remove_dir_all(git_dir)?;
        }
        Ok(())
    }
    pub fn create_patch(repo_dir: &Path, patch_file: &Path) -> anyhow::Result<()> {
        Command::new("git")
            .current_dir(repo_dir)
            .args(["add", "."])
            .output()?;

        let out = Command::new("git")
            .current_dir(repo_dir)
            .args([
                OsStr::new("diff"),
                OsStr::new("--staged"),
                OsStr::new("--no-ext-diff"),
            ])
            .output()?;

        if out.status.success() {
            fs::write(patch_file, out.stdout)?;
        }
        Ok(())
    }
}
