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

use anyhow::{Ok, Result, anyhow, bail};
use clap::Parser;
use fs_extra::dir::{CopyOptions, copy};
use log::*;
use serde::Deserialize;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

mod manifest;

const PATCH_EXT: &str = "patch";

#[derive(Parser, Debug, Default)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    pub crates: Vec<String>,
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<LockPkg>,
}

#[derive(Deserialize)]
struct LockPkg {
    name: String,
    version: String,
}

#[derive(Clone, Hash, Eq, PartialEq, Debug)]
struct CrateRef {
    name: String,
    version: String,
}

impl CrateRef {
    fn slug(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }
}

fn load_lock(ws_root: &Path) -> Result<CargoLock> {
    let path = ws_root.join("Cargo.lock");
    let text = fs::read_to_string(&path)
        .map_err(|e| anyhow!("failed to read {:?}: {}", path, e))?;
    toml::from_str(&text).map_err(|e| anyhow!("failed to parse Cargo.lock: {}", e))
}

fn resolve_crate(lock: &CargoLock, spec: &str) -> Result<CrateRef> {
    let (name, want_ver) = match spec.split_once('@') {
        Some((n, v)) => (n, Some(v)),
        None => (spec, None),
    };
    // Accept any lockfile entry by name. A [patch.crates-io] redirect rewrites the
    // `source` field to None, so filtering on `registry+` drops the crate we want
    // to compare against. `find_crate_src` handles non-registry crates by failing
    // to locate them in the registry cache and reporting a clear error.
    let matches: Vec<&LockPkg> = lock
        .package
        .iter()
        .filter(|p| p.name == name && want_ver.is_none_or(|v| p.version == v))
        .collect();
    match matches.len() {
        0 => Err(anyhow!("package `{}` not found in Cargo.lock", spec)),
        1 => Ok(CrateRef {
            name: matches[0].name.clone(),
            version: matches[0].version.clone(),
        }),
        _ => Err(anyhow!(
            "multiple versions of `{}` found; specify with name@version",
            name
        )),
    }
}

fn find_crate_src(ws_root: &Path, cr: &CrateRef) -> Result<PathBuf> {
    let registry = home::cargo_home()?.join("registry/src");
    if registry.is_dir() {
        for entry in fs::read_dir(&registry)? {
            let p = entry?.path().join(cr.slug());
            if p.is_dir() {
                return Ok(p);
            }
        }
    }
    download_crate(ws_root, cr)
}

fn download_crate(ws_root: &Path, cr: &CrateRef) -> Result<PathBuf> {
    let cache = ws_root.join("target/patch-download");
    fs::create_dir_all(&cache)?;
    let dst = cache.join(cr.slug());
    if dst.is_dir() {
        return Ok(dst);
    }
    let url = format!(
        "https://static.crates.io/crates/{0}/{0}-{1}.crate",
        cr.name, cr.version
    );
    info!("downloading {}", url);
    let resp = ureq::get(&url)
        .set(
            "User-Agent",
            "cargo-patch-crate (https://github.com/jspaezp/cargo-patch-crate)",
        )
        .call()
        .map_err(|e| anyhow!("download failed for {}: {}", url, e))?;
    let gz = flate2::read::GzDecoder::new(resp.into_reader());
    tar::Archive::new(gz)
        .unpack(&cache)
        .map_err(|e| anyhow!("extract failed for {}: {}", url, e))?;
    if !dst.is_dir() {
        bail!("extracted archive did not produce {:?}", dst);
    }
    Ok(dst)
}

fn copy_crate(src: &Path, dst_folder: &Path, overwrite: bool) -> Result<PathBuf> {
    fs::create_dir_all(dst_folder)?;
    let slug = src
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("source folder has no name: {:?}", src))?;
    let dst = dst_folder.join(slug);
    if dst.exists() {
        if overwrite {
            info!("copy {} to {:?}", slug, dst_folder);
            fs::remove_dir_all(&dst)?;
        } else {
            info!("skip {}, {:?} already exists.", slug, dst);
            return Ok(dst);
        }
    }
    copy(src, dst_folder, &CopyOptions::new())?;
    Ok(dst)
}

fn patches_folder(ws_root: &Path) -> PathBuf {
    ws_root.join("patches")
}

fn patch_target_folder(ws_root: &Path) -> PathBuf {
    ws_root.join("target/patch")
}

fn patch_target_tmp_folder(ws_root: &Path) -> PathBuf {
    ws_root.join("target/patch-tmp")
}

fn clean_patch_folder(ws_root: &Path) -> Result<()> {
    let p = patch_target_folder(ws_root);
    if p.exists() {
        fs::remove_dir_all(p)?;
    }
    Ok(())
}

fn resolve_patch_list(
    patch_crate_names: &[String],
    lock: &CargoLock,
) -> Result<HashSet<CrateRef>> {
    let mut list = HashSet::new();
    for name in patch_crate_names {
        list.insert(resolve_crate(lock, name)?);
    }
    Ok(list)
}

pub fn run_at(ws_hint: &Path, cli: Cli) -> Result<()> {
    let ws = manifest::discover(ws_hint)?;
    let lock = load_lock(&ws.root)?;

    let patches_folder = patches_folder(&ws.root);
    let patch_target_folder = patch_target_folder(&ws.root);
    let patch_target_tmp_folder = patch_target_tmp_folder(&ws.root);

    if !cli.crates.is_empty() {
        info!("starting patch creation.");
        if !patches_folder.exists() {
            fs::create_dir_all(&patches_folder)?;
        }
        for n in cli.crates.iter() {
            info!("crate: {}, starting patch creation.", n);
            let cr = resolve_crate(&lock, n)?;
            let src = find_crate_src(&ws.root, &cr)?;
            let patched_crate_path = patch_target_folder.join(cr.slug());

            let original_crate_path = copy_crate(&src, &patch_target_tmp_folder, true)?;
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
                patches_folder.join(format!("{}+{}.{}", cr.name, cr.version, PATCH_EXT));
            git::create_patch(&patched_crate_path, &patch_file)?;
            fs::remove_dir_all(&patch_target_tmp_folder)?;

            git::destroy(&patched_crate_path)?;
            info!("crate: {}, create patch successfully, {:?}", n, &patch_file);
        }
    } else {
        info!("applying patch");

        let mut crates_to_patch = resolve_patch_list(&ws.patch_crates, &lock)?;

        if cli.force {
            info!("Cleaning up patch folder.");
            clean_patch_folder(&ws.root)?;
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
                        let cr = CrateRef {
                            name: pkg_name.to_string(),
                            version: version.to_string(),
                        };
                        if !crates_to_patch.contains(&cr) {
                            warn!(
                                "crate: {}, {} is not in the [package.metadata.patch] or [workspace.metadata.patch] section of Cargo.toml. Did you forget to add it?",
                                pkg_name, pkg_name
                            );
                            continue;
                        }
                        let target = patch_target_folder.join(cr.slug());
                        if !target.exists() {
                            let src = find_crate_src(&ws.root, &cr)?;
                            copy_crate(&src, &patch_target_folder, cli.force)?;
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
                        crates_to_patch.remove(&cr);
                    }
                }
            }
        }
        for cr in crates_to_patch {
            let src = find_crate_src(&ws.root, &cr)?;
            copy_crate(&src, &patch_target_folder, cli.force)?;
        }
    }

    info!("Done");
    Ok(())
}

pub fn run() -> Result<()> {
    let mut cli = Cli::parse();
    if let Some(idx) = cli.crates.iter().position(|c| c == "patch-crate") {
        cli.crates.remove(idx);
    }
    let cwd = std::env::current_dir()?;
    run_at(&cwd, cli)
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
