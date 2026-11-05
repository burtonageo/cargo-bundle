//! Checks that apply to every format.

use std::fs;

mod common;
use common::project;

#[test]
fn an_unusable_bundle_name_fails_before_anything_is_written() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(temp.path(), "");
    let manifest = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        manifest.replace("Post Card", "../../escape"),
    )
    .unwrap();
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "binary").unwrap();

    for format in ["osx", "dmg", "ios", "deb", "appimage"] {
        let output = bundle(&["--format", format], &binary);
        assert!(!output.status.success(), "{format}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("must be a single file name"), "{stderr}");
    }
    assert!(!temp.path().join("target").exists());
    assert!(!temp.path().parent().unwrap().join("escape").exists());
}
