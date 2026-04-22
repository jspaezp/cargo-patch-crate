mod common;

use patch_crate::Cli;

#[test]
fn bootstrap_applies_patch_to_filesystem() {
    let (_tmp, dir) = common::stage_fixture("roundtrip");
    patch_crate::run_at(&dir, Cli::default()).expect("reconcile");

    let patch_root = dir.join("target/patch");
    let mut home_dir: Option<std::path::PathBuf> = None;
    for entry in std::fs::read_dir(&patch_root).expect("read target/patch") {
        let p = entry.expect("dirent").path();
        if p.file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.starts_with("home-"))
        {
            home_dir = Some(p);
            break;
        }
    }
    let home_dir = home_dir.expect("home-<version> directory present");
    let src = std::fs::read_to_string(home_dir.join("src/lib.rs")).expect("lib.rs");
    assert!(
        src.contains("i_was_patched_correctly"),
        "patched function missing from home/src/lib.rs:\n{}",
        src
    );
}

#[test]
fn reconcile_is_idempotent() {
    let (_tmp, dir) = common::stage_fixture("roundtrip");
    let patches_dir = dir.join("patches");

    patch_crate::run_at(&dir, Cli::default()).expect("reconcile 1");
    let snapshot_before = snapshot_dir(&patches_dir);

    patch_crate::run_at(&dir, Cli::default()).expect("reconcile 2");
    let snapshot_after = snapshot_dir(&patches_dir);

    assert_eq!(
        snapshot_before, snapshot_after,
        "patches/ changed between idempotent reconcile runs"
    );
}

#[test]
#[ignore = "runs nested cargo; network + slow"]
fn end_to_end_cargo_run_prints_patched_string() {
    let (_tmp, dir) = common::stage_fixture("roundtrip");
    patch_crate::run_at(&dir, Cli::default()).expect("reconcile");

    let out = std::process::Command::new("cargo")
        .current_dir(&dir)
        .args(["run", "--quiet"])
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("cargo run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "cargo run failed:\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(
        stdout.contains("i was patched correctly"),
        "expected patched output, got:\n{}",
        stdout
    );
}

fn snapshot_dir(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dirent");
        if entry.metadata().expect("meta").is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            let bytes = std::fs::read(entry.path()).expect("read file");
            out.push((name, bytes));
        }
    }
    out.sort();
    out
}
