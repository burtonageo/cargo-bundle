mod common;

use std::process::Command;

#[test]
#[cfg(target_os = "macos")]
fn osx() {
    use std::fs;
    common::setup_example_binary("hello");

    let root = common::project_root();
    let output = Command::new(common::cargo_bundle_bin())
        .args(["bundle", "--example", "hello", "--format", "osx"])
        .current_dir(&root)
        .env("CARGO_BUNDLE_SKIP_BUILD", "1")
        .output()
        .expect("Failed to execute cargo-bundle");

    assert!(
        output.status.success(),
        "cargo-bundle failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let app_path = root.join("target/debug/examples/bundle/osx/hello.app");
    assert!(app_path.exists(), "App bundle not found at {:?}", app_path);

    // SVG icon should have been converted to ICNS.
    let icns_path = app_path.join("Contents/Resources/hello.icns");
    assert!(icns_path.exists(), "ICNS not found at {:?}", icns_path);
    assert!(
        fs::metadata(&icns_path).unwrap().len() > 0,
        "ICNS file is empty"
    );
}

#[test]
fn deb() {
    common::setup_example_binary("hello");

    let root = common::project_root();
    let output = Command::new(common::cargo_bundle_bin())
        .args(["bundle", "--example", "hello", "--format", "deb"])
        .current_dir(&root)
        .env("CARGO_BUNDLE_SKIP_BUILD", "1")
        .output()
        .expect("Failed to execute cargo-bundle");

    assert!(
        output.status.success(),
        "cargo-bundle failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle_paths = common::parse_bundle_paths(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(bundle_paths.len(), 1, "Expected exactly one bundle path");
    let deb_path = &bundle_paths[0];
    assert!(
        deb_path.exists(),
        "Debian package not found at {:?}",
        deb_path
    );

    // SVG should be copied to the scalable hicolor directory.
    let package_dir = deb_path
        .parent()
        .unwrap()
        .join(deb_path.file_stem().unwrap());
    let svg_path = package_dir.join("data/usr/share/icons/hicolor/scalable/apps/hello.svg");
    assert!(
        svg_path.exists(),
        "SVG icon not found in deb data at {:?}",
        svg_path
    );
}

#[test]
fn deb_uses_prebuilt_binary_without_running_cargo_build() {
    common::setup_example_binary("hello");

    let root = common::project_root();
    let binary = root.join("target/debug/examples/hello");
    let output = Command::new(common::cargo_bundle_bin())
        .args([
            "bundle",
            "--example",
            "hello",
            "--format",
            "deb",
            // This target is valid package metadata, but is not installed on
            // the test host. A cargo build would fail before packaging.
            "--target",
            "aarch64-unknown-linux-gnu",
            "--binary-path",
        ])
        .arg(&binary)
        .current_dir(&root)
        .output()
        .expect("Failed to execute cargo-bundle");

    assert!(
        output.status.success(),
        "cargo-bundle failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle_paths = common::parse_bundle_paths(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(bundle_paths.len(), 1, "Expected exactly one bundle path");
    assert!(bundle_paths[0].exists(), "Debian package was not created");
}
