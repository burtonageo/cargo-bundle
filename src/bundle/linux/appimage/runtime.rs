//! The ELF runtime at the front of an AppImage.
//!
//! By default this is the launcher from the `appimage-rs` crate, built with
//! cargo for the target and cached: no prebuilt binary is downloaded or
//! shipped, and the result needs neither FUSE nor any helper program on the
//! user's machine. `appimage_runtime` replaces it with a runtime you supply.

use crate::bundle::{Settings, common};
use anyhow::{Context, bail};
use appimage::Runtime;
use goblin::elf::{Elf, header};
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

/// The `appimage-rs` requirement the launcher is built from. It must match the
/// dependency in `Cargo.toml`, which a test checks.
const APPIMAGE_RS_REQUIREMENT: &str = "0.3.2";

/// Load the runtime for `architecture` (a name from `arch.rs`).
pub(super) fn read(settings: &Settings, architecture: &str) -> crate::Result<Runtime> {
    if let Some(path) = settings.appimage_runtime() {
        let bytes =
            fs::read(path).with_context(|| format!("Failed to read AppImage runtime {path:?}"))?;
        return parse(bytes, architecture)
            .with_context(|| format!("Invalid appimage_runtime {path:?}"));
    }

    let triple = settings.target_triples().next();
    let cached = cache_path(triple, architecture);
    if let Ok(bytes) = fs::read(&cached)
        && let Ok(runtime) = parse(bytes, architecture)
    {
        return Ok(runtime);
    }

    let runtime = parse(build_launcher(triple)?, architecture).with_context(|| {
        format!(
            "The launcher built for {} is not an executable for {architecture}; \
             set appimage_runtime to a runtime built for the target",
            triple.unwrap_or("the host")
        )
    })?;

    // The cache saves the next build a compile; a read-only cache is not an error.
    let cached_ok = cached
        .parent()
        .map(fs::create_dir_all)
        .transpose()
        .and_then(|_| fs::write(&cached, &*runtime));
    if let Err(error) = cached_ok {
        common::print_warning(&format!(
            "Could not cache the AppImage runtime at {cached:?}: {error}"
        ))?;
    }
    Ok(runtime)
}

/// Where a built launcher is kept: one file per `appimage-rs` version and target.
fn cache_path(triple: Option<&str>, architecture: &str) -> PathBuf {
    let target = triple.map_or_else(|| format!("host-{architecture}"), str::to_owned);
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cargo-bundle/appimage-runtime")
        .join(APPIMAGE_RS_REQUIREMENT)
        .join(target)
}

/// Compiles the launcher for `triple` (or the host) with the user's own cargo.
///
/// The target is the one the application is being built for, so the standard
/// library and linker it needs are already set up. `appimage-rs` is resolved
/// offline first: cargo-bundle depends on it with the `launcher` feature, so
/// installing cargo-bundle already put its sources in cargo's registry.
fn build_launcher(triple: Option<&str>) -> crate::Result<Vec<u8>> {
    let project = tempfile::tempdir().context("Failed to create a build directory")?;
    fs::create_dir(project.path().join("src"))?;
    fs::write(project.path().join("src/main.rs"), LAUNCHER_MAIN)?;
    fs::write(project.path().join("Cargo.toml"), launcher_manifest())?;

    common::print_progress(
        "Building",
        &format!("the AppImage launcher for {}", triple.unwrap_or("the host")),
    )?;
    let run = |offline: bool| {
        let mut cargo = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        cargo
            .args(["build", "--release", "--quiet", "--manifest-path"])
            .arg(project.path().join("Cargo.toml"))
            .arg("--target-dir")
            .arg(project.path().join("target"));
        if let Some(triple) = triple {
            cargo.args(["--target", triple]);
        }
        if offline {
            cargo.arg("--offline");
        }
        cargo.output().context("Failed to run cargo")
    };
    let mut output = run(true)?;
    if !output.status.success() {
        output = run(false)?;
    }
    if !output.status.success() {
        bail!(
            "Failed to build the AppImage launcher for {}. Install that target for your \
             toolchain, or set appimage_runtime to a runtime you built.\n{}",
            triple.unwrap_or("the host"),
            String::from_utf8_lossy(&output.stderr).trim_end()
        );
    }

    let mut path = project.path().join("target");
    path.extend(triple);
    let launcher = path.join("release/appimage-runtime");
    fs::read(&launcher).with_context(|| format!("The launcher was not produced at {launcher:?}"))
}

const LAUNCHER_MAIN: &str =
    "fn main() -> std::process::ExitCode {\n    appimage::launcher::main()\n}\n";

/// A small package whose only job is to build the launcher, optimized for
/// size, since a copy of it is the front of every AppImage.
fn launcher_manifest() -> String {
    format!(
        "[package]\nname = \"appimage-runtime\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
         [dependencies]\nappimage = {{ package = \"appimage-rs\", version = \"{APPIMAGE_RS_REQUIREMENT}\", \
         features = [\"launcher\"] }}\n\n\
         [profile.release]\nopt-level = \"z\"\nlto = true\ncodegen-units = 1\npanic = \"abort\"\nstrip = true\n\n\
         [workspace]\n"
    )
}

/// Accept `bytes` only if they are an ELF executable for `architecture`.
///
/// Packing an x86-64 runtime into an aarch64 image would build without error
/// and then fail on every user's machine, so the machine type is checked here.
fn parse(bytes: Vec<u8>, architecture: &str) -> crate::Result<Runtime> {
    let expected = match architecture {
        "x86_64" => header::EM_X86_64,
        "aarch64" => header::EM_AARCH64,
        "i686" => header::EM_386,
        "armhf" => header::EM_ARM,
        other => bail!("No AppImage runtime exists for architecture {other:?}"),
    };
    let machine = Elf::parse_header(&bytes)
        .context("Not an ELF executable")?
        .e_machine;
    if machine != expected {
        bail!("ELF machine type {machine} does not match {architecture} (expected {expected})");
    }
    Ok(Runtime::try_from(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf(machine: u16) -> Vec<u8> {
        let mut bytes = Runtime::inert().to_vec();
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn accepts_only_an_elf_for_the_target_architecture() {
        assert!(parse(elf(header::EM_AARCH64), "aarch64").is_ok());
        assert!(parse(elf(header::EM_X86_64), "x86_64").is_ok());
        assert!(parse(elf(header::EM_X86_64), "aarch64").is_err());
        assert!(parse(b"#!/bin/sh\n".to_vec(), "x86_64").is_err());
        assert!(parse(Vec::new(), "x86_64").is_err());
    }

    #[test]
    fn the_built_launcher_uses_the_dependency_version_from_cargo_toml() {
        let manifest =
            fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
        let line = manifest
            .lines()
            .find(|line| line.starts_with("appimage = "))
            .unwrap();
        assert!(
            line.contains(&format!("version = \"{APPIMAGE_RS_REQUIREMENT}\"")),
            "{line}"
        );
        assert!(launcher_manifest().contains(APPIMAGE_RS_REQUIREMENT));
    }

    /// Builds the real launcher for the host. Slow, so run it on demand:
    /// `cargo test -- --ignored builds_the_launcher`.
    #[test]
    #[ignore]
    fn builds_the_launcher() {
        let launcher = build_launcher(None).unwrap();
        println!("launcher size: {} bytes", launcher.len());
        assert!(!launcher.is_empty());
    }
}
