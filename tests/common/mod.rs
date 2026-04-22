use std::path::{Path, PathBuf};

pub fn stage_fixture(name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    fs_extra::dir::copy(
        &src,
        tmp.path(),
        &fs_extra::dir::CopyOptions::new().copy_inside(true),
    )
    .expect("copy fixture");
    let dst = tmp.path().join(name);
    (tmp, dst)
}
