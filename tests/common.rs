//! Helpers shared by the integration tests; each test crate uses a subset.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn cargo_bundle_bin() -> PathBuf {
    project_root().join("target/debug/cargo-bundle")
}

#[cfg(test)]
#[allow(dead_code)] // Used in tests
pub fn parse_bundle_paths(stdout: &str) -> Vec<PathBuf> {
    stdout
        .lines()
        .filter(|line| line.starts_with("        "))
        .map(|line| PathBuf::from(line.trim()))
        .collect()
}

/// Places a valid-enough binary at `target/debug/examples/<name>` so that
/// cargo-bundle (running with CARGO_BUNDLE_SKIP_BUILD) has something to package.
pub fn setup_example_binary(name: &str) {
    let bin_dir = project_root().join("target/debug/examples");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin_path = bin_dir.join(name);
    #[cfg(target_os = "macos")]
    {
        // macOS copies the binary into the .app; use a for realsies Mach-O so the copy runs.
        fs::copy(cargo_bundle_bin(), &bin_path).unwrap();
    }
    #[cfg(not(target_os = "macos"))]
    {
        fs::write(&bin_path, b"dummy binary content").unwrap();
    }
}

/// Writes a package whose `[package.metadata.bundle]` table holds `bundle`,
/// and returns a runner for `cargo bundle` in `dir`.
pub fn project(dir: &Path, bundle: &str) -> impl Fn(&[&str], &Path) -> Output {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"postcard\"\nversion = \"1.2.3\"\nedition = \"2024\"\n\
             [package.metadata.bundle]\nname = \"Post Card\"\nidentifier = \"com.example.postcard\"\n\
             {bundle}"
        ),
    )
    .unwrap();
    let dir = dir.to_owned();
    move |args, binary| {
        Command::new(env!("CARGO_BIN_EXE_cargo-bundle"))
            .arg("bundle")
            .args(args)
            .arg("--binary-path")
            .arg(binary)
            .current_dir(&dir)
            .output()
            .unwrap()
    }
}

pub fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
