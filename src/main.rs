mod bundle;

use crate::bundle::{BuildArtifact, PackageType, Settings, bundle_project};
use anyhow::Result;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use std::env;
use std::ffi::OsString;
use std::process;

#[macro_export]
macro_rules! version_0 {
    () => {
        concat!("v", clap::crate_version!())
    };
}

#[macro_export]
macro_rules! version_info {
    () => {
        concat!(clap::crate_name!(), " ", $crate::version_0!())
    };
}

fn about_info() -> String {
    format!(
        "{}\n{}\n{}",
        version_info!(),
        clap::crate_authors!(", "),
        "Bundle Rust executables into OS bundles",
    )
}

#[derive(clap::Parser, Clone)]
#[command(version = version_0!(), author = clap::crate_authors!(", "), bin_name = "cargo bundle", about = about_info())]
pub struct Cli {
    /// Bundle the specified binary
    #[arg(short, long, value_name = "NAME")]
    pub bin: Option<String>,

    /// Bundle the specified example
    #[arg(short, long, value_name = "NAME", conflicts_with = "bin")]
    pub example: Option<String>,

    /// Which bundle format to produce
    #[arg(short, long, value_name = "FORMAT", value_parser = PossibleValuesParser::new(PackageType::all()).map(|s| PackageType::try_from(s).unwrap()))]
    pub format: Option<PackageType>,

    /// Build a bundle from a target built in release mode
    #[arg(short, long)]
    pub release: bool,

    /// Build a bundle from a target build using the given profile
    #[arg(long, value_name = "NAME", conflicts_with = "release")]
    pub profile: Option<String>,

    /// Build a bundle for the target triple. May be repeated to combine
    /// several architectures into a universal binary (macOS only).
    #[arg(short, long, value_name = "TRIPLE")]
    pub target: Vec<String>,

    /// Set crate features for the bundle. Eg: `--features "f1 f2"`
    #[arg(long, value_name = "FEATURES")]
    pub features: Option<String>,

    /// Build a bundle with all crate features.
    #[arg(long)]
    pub all_features: bool,

    /// Build a bundle without the default crate features.
    #[arg(long)]
    pub no_default_features: bool,

    /// The name of the package to bundle. If not specified, the root package will be used.
    #[arg(short, long, value_name = "SPEC")]
    pub package: Option<String>,
}

/// Runs `cargo build` to make sure the binary file is up-to-date.
///
/// When several target triples were requested, builds each one and then
/// combines the resulting binaries into a universal binary with `lipo`.
fn build_project_if_unbuilt(settings: &Settings) -> crate::Result<()> {
    if std::env::var("CARGO_BUNDLE_SKIP_BUILD").is_ok() {
        return Ok(());
    }

    let mut triples: Vec<Option<&str>> = settings.target_triples().map(Some).collect();
    if triples.is_empty() {
        triples.push(None);
    }
    for triple in triples {
        build_target(settings, triple)?;
    }
    combine_universal_binary(settings)?;
    Ok(())
}

/// Runs a single `cargo build`, optionally for an explicit target triple.
fn build_target(settings: &Settings, triple: Option<&str>) -> crate::Result<()> {
    let mut cargo =
        process::Command::new(env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")));
    cargo.arg("build");
    if let Some(triple) = triple {
        cargo.arg(format!("--target={triple}"));
    }
    if let Some(features) = settings.features() {
        cargo.arg(format!("--features={features}"));
    }
    match settings.build_artifact() {
        BuildArtifact::Main => {}
        BuildArtifact::Bin(name) => {
            cargo.arg(format!("--bin={name}"));
        }
        BuildArtifact::Example(name) => {
            cargo.arg(format!("--example={name}"));
        }
    }
    match settings.build_profile() {
        "dev" => {}
        "release" => {
            cargo.arg("--release");
        }
        custom => {
            cargo.arg("--profile");
            cargo.arg(custom);
        }
    }
    if settings.all_features() {
        cargo.arg("--all-features");
    }
    if settings.no_default_features() {
        cargo.arg("--no-default-features");
    }
    let status = cargo.status()?;
    if !status.success() {
        anyhow::bail!(
            "Result of `cargo build` operation was unsuccessful: {}",
            status
        );
    }
    Ok(())
}

/// Merges the per-target binaries into one universal binary with `lipo`.
/// Does nothing when fewer than two target triples were requested.
fn combine_universal_binary(settings: &Settings) -> crate::Result<()> {
    let input_binary_paths = settings.universal_input_binary_paths();
    if input_binary_paths.is_empty() {
        return Ok(());
    }
    let output_binary_path = settings.binary_path();
    if let Some(parent) = output_binary_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = process::Command::new("lipo")
        .arg("-create")
        .arg("-output")
        .arg(output_binary_path)
        .args(input_binary_paths)
        .status()
        .map_err(|error| {
            anyhow::anyhow!("Failed to run `lipo` (universal binaries require macOS): {error}")
        })?;
    if !status.success() {
        anyhow::bail!("Result of `lipo` operation was unsuccessful: {}", status);
    }
    Ok(())
}

fn run() -> crate::Result<()> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "bundle" {
        args.remove(1);
    }
    let cli = <Cli as clap::Parser>::parse_from(args); // <Cli as clap::Parser>::parse();

    {
        let output_paths = env::current_dir()
            .map_err(From::from)
            .and_then(|d| Settings::new(d, &cli))
            .and_then(|s| {
                build_project_if_unbuilt(&s)?;
                Ok(s)
            })
            .and_then(bundle_project)?;
        bundle::print_finished(&output_paths)?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        bundle::print_error(&error).unwrap();
        std::process::exit(1);
    }
}
