//! Builds an AppImage through the CLI. The runtime is an inert ELF given with
//! `appimage_runtime`, so the test needs no network and runs on any host.

use appimage::{AppImage, Runtime};
use std::fs;

mod common;
use common::{project, succeeded};

const TARGET: &str = "aarch64-unknown-linux-gnu";

/// The runtime's machine type is stored at byte 18 of the ELF header.
fn runtime(machine: u16) -> Vec<u8> {
    let mut bytes = Runtime::inert().to_vec();
    bytes[18..20].copy_from_slice(&machine.to_le_bytes());
    bytes
}

#[test]
fn appimage_wraps_the_binary_behind_the_given_runtime_and_survives_failures() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(
        temp.path(),
        "appimage_runtime = \"runtime\"\nappimage_compression = \"gzip\"\n",
    );
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, include_bytes!("fixtures/android/libpostcard.so")).unwrap();
    fs::write(temp.path().join("runtime"), runtime(183)).unwrap();
    let args = ["--format", "appimage", "--target", TARGET];

    let built = bundle(&args, &binary);
    succeeded(&built);
    let output = temp
        .path()
        .join("target")
        .join(TARGET)
        .join("debug/bundle/appimage/postcard_1.2.3_aarch64.AppImage");
    let bytes = fs::read(&output).unwrap();
    assert!(bytes.starts_with(&runtime(183)[..64]), "runtime is first");
    assert_eq!(&bytes[8..11], b"AI\x02");

    let extracted = temp.path().join("extracted");
    AppImage::open(&output)
        .unwrap()
        .unpack()
        .to(&extracted)
        .unwrap();
    assert_eq!(
        fs::read(extracted.join("usr/bin/postcard")).unwrap(),
        fs::read(&binary).unwrap()
    );
    // AppRun is a link, which needs a privilege on Windows to extract.
    #[cfg(unix)]
    assert!(extracted.join("AppRun").exists());

    // A runtime for the wrong CPU would build and then never start.
    fs::write(temp.path().join("runtime"), runtime(62)).unwrap();
    let wrong_machine = bundle(&args, &binary);
    assert!(!wrong_machine.status.success());
    let stderr = String::from_utf8_lossy(&wrong_machine.stderr);
    assert!(stderr.contains("does not match aarch64"), "{stderr}");
    assert_eq!(fs::read(&output).unwrap(), bytes, "old image is untouched");

    fs::write(temp.path().join("runtime"), runtime(183)).unwrap();
    let manifest = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        manifest.replace("\"gzip\"", "\"zip\""),
    )
    .unwrap();
    let bad_codec = bundle(&args, &binary);
    assert!(!bad_codec.status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes, "old image is untouched");

    // Rebuilding replaces the earlier image.
    fs::write(temp.path().join("Cargo.toml"), manifest).unwrap();
    let mut changed = fs::read(&binary).unwrap();
    changed.push(0);
    fs::write(&binary, &changed).unwrap();
    succeeded(&bundle(&args, &binary));
    let replaced = temp.path().join("replaced");
    AppImage::open(&output)
        .unwrap()
        .unpack()
        .to(&replaced)
        .unwrap();
    assert_eq!(
        fs::read(replaced.join("usr/bin/postcard")).unwrap(),
        changed
    );
}

/// Without `appimage_runtime` the launcher is built from source for the target
/// and put in front of the image; running the image must run the app. Linux
/// only, because the image has to execute, and it needs cargo to build a crate.
#[cfg(target_os = "linux")]
#[test]
fn the_default_runtime_is_built_from_source_and_runs_the_app() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    // Only the manifest is needed; the bundle is built by hand to set the cache.
    let _ = project(temp.path(), "");
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "#!/bin/sh\necho \"launched with $*\"\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-bundle"))
        .args(["bundle", "--format", "appimage", "--binary-path"])
        .arg(&binary)
        .current_dir(temp.path())
        .env("XDG_CACHE_HOME", &cache)
        .env("TERM", "dumb")
        .output()
        .unwrap();
    succeeded(&output);

    let image = temp.path().join(format!(
        "target/debug/bundle/appimage/postcard_1.2.3_{}.AppImage",
        std::env::consts::ARCH
    ));
    let run = Command::new(&image).arg("one").arg("two").output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "launched with one two\n"
    );
    // It is a copy of the launcher, not of the official FUSE runtime.
    let versions = cache.join("cargo-bundle/appimage-runtime");
    let version = fs::read_dir(&versions).unwrap().next().unwrap().unwrap();
    let launcher = fs::read(
        version
            .path()
            .join(format!("host-{}", std::env::consts::ARCH)),
    )
    .unwrap();
    assert!(fs::read(&image).unwrap().starts_with(&launcher));
}
