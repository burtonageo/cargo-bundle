use clap::ArgMatches;
use std;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use super::common::print_warning;
use target_build_utils::TargetInfo;
use toml;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageType {
    OsxBundle,
    IosBundle,
    Deb,
    Rpm,
}

#[derive(Clone, Debug, Deserialize)]
struct BundleSettings {
    name: Option<String>,
    identifier: Option<String>,
    icon: Option<Vec<PathBuf>>,
    version: Option<String>,
    resources: Option<Vec<PathBuf>>,
    copyright: Option<String>,
    short_description: Option<String>,
    long_description: Option<String>,
    script: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
struct MetadataSettings {
    bundle: Option<BundleSettings>,
}

#[derive(Clone, Debug, Deserialize)]
struct PackageSettings {
    name: String,
    version: String,
    description: String,
    homepage: Option<String>,
    authors: Option<Vec<String>>,
    metadata: Option<MetadataSettings>,
}

#[derive(Clone, Debug, Deserialize)]
struct CargoSettings {
    package: PackageSettings,
}

#[derive(Clone, Debug)]
pub struct Settings {
    cargo_settings: CargoSettings,
    package_type: Option<PackageType>, // If `None`, use the default package type for this os
    target: Option<(String, TargetInfo)>,
    project_out_directory: PathBuf,
    is_release: bool,
    binary_path: PathBuf,
    bundle_settings: BundleSettings,
}

impl Settings {
    pub fn new(current_dir: PathBuf, matches: &ArgMatches) -> ::Result<Self> {
        let package_type = match matches.value_of("format") {
            // Other types we may eventually want to support: apk, win
            None => None,
            Some("deb") => Some(PackageType::Deb),
            Some("ios") => Some(PackageType::IosBundle),
            Some("osx") => Some(PackageType::OsxBundle),
            Some("rpm") => Some(PackageType::Rpm),
            Some(format) => bail!("Unsupported bundle format: {}", format),
        };
        let is_release = matches.is_present("release");
        let target = match matches.value_of("target") {
            Some(triple) => Some((triple.to_string(), TargetInfo::from_str(triple)?)),
            None => None,
        };
        let target_dir = {
            let mut path = current_dir.join("target");
            if let Some((ref triple, _)) = target {
                path.push(triple);
            }
            path.push(if is_release { "release" } else { "debug" });
            path
        };
        let cargo_settings: CargoSettings = {
            let toml_path = current_dir.join("Cargo.toml");
            let mut toml_str = String::new();
            let mut toml_file = File::open(toml_path)?;
            toml_file.read_to_string(&mut toml_str)?;
            toml::from_str(&toml_str)?
        };
        let binary_path = target_dir.join(&cargo_settings.package.name);
        let bundle_settings: BundleSettings = {
            let toml_path = current_dir.join("Bundle.toml");
            if toml_path.exists() {
                print_warning(BUNDLE_TOML_WARNING)?;
                let mut toml_str = String::new();
                let mut toml_file = File::open(toml_path)?;
                toml_file.read_to_string(&mut toml_str)?;
                toml::from_str(&toml_str)?
            } else if let Some(bundle_settings) = cargo_settings.package.metadata.as_ref().and_then(|metadata| metadata.bundle.as_ref()) {
                bundle_settings.clone()
            } else {
                bail!("No [package.metadata.bundle] section or ]Bundle.toml file found.");
            }
        };
        Ok(Settings {
            cargo_settings: cargo_settings,
            package_type: package_type,
            target: target,
            is_release: is_release,
            project_out_directory: target_dir,
            binary_path: binary_path,
            bundle_settings: bundle_settings,
        })
    }

    /// Returns the directory where the bundle should be placed.
    pub fn project_out_directory(&self) -> &Path {
        &self.project_out_directory
    }

    /// Returns the architecture for the binary being bundled (e.g. "arm" or
    /// "x86" or "x86_64").
    pub fn binary_arch(&self) -> &str {
        if let Some((_, ref info)) = self.target {
            info.target_arch()
        } else {
            std::env::consts::ARCH
        }
    }

    /// Returns the file name of the binary being bundled.
    pub fn binary_name(&self) -> ::Result<String> {
        self.binary_path
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToString::to_string)
            .ok_or("Could not get file name of binary file.".into())
    }

    /// Returns the path to the binary being bundled.
    pub fn binary_path(&self) -> &Path { &self.binary_path }

    /// If a specific package type was specified by the command-line, returns
    /// that package type; otherwise, if a target triple was specified by the
    /// command-line, returns the native package type(s) for that target;
    /// otherwise, returns the native package type(s) for the host platform.
    /// Fails if the host/target's native package type is not supported.
    pub fn package_types(&self) -> ::Result<Vec<PackageType>> {
        if let Some(package_type) = self.package_type {
            Ok(vec![package_type])
        } else {
            let target_os = if let Some((_, ref info)) = self.target {
                info.target_os()
            } else {
                std::env::consts::OS
            };
            match target_os {
                "macos" => Ok(vec![PackageType::OsxBundle]),
                "ios" => Ok(vec![PackageType::IosBundle]),
                "linux" => Ok(vec![PackageType::Deb]), // TODO: Do Rpm too, once it's implemented.
                os => bail!("Native {} bundles not yet supported.", os),
            }
        }
    }

    /// If the bundle is being cross-compiled, returns the target triple string
    /// (e.g. `"x86_64-apple-darwin"`).  If the bundle is targeting the host
    /// environment, returns `None`.
    pub fn target_triple(&self) -> Option<&str> {
        match self.target {
            Some((ref triple, _)) => Some(triple.as_str()),
            None => None,
        }
    }

    /// Returns true if the bundle is being compiled in release mode, false if
    /// it's being compiled in debug mode.
    pub fn is_release_build(&self) -> bool { self.is_release }

    pub fn bundle_name(&self) -> &str {
        self.bundle_settings.name.as_ref().unwrap_or(&self.cargo_settings.package.name)
    }

    pub fn bundle_identifier(&self) -> &str {
        self.bundle_settings.identifier.as_ref().map(String::as_str).unwrap_or("")
    }

    /// Returns an iterator over the icon files to be used for this bundle.
    pub fn icon_files(&self) -> std::slice::Iter<PathBuf> {
        match self.bundle_settings.icon {
            Some(ref paths) => paths.iter(),
            None => [].iter(),
        }
    }

    /// Returns an iterator over the resource files to be included in this
    /// bundle.
    pub fn resource_files(&self) -> std::slice::Iter<PathBuf> {
        match self.bundle_settings.resources {
            Some(ref paths) => paths.iter(),
            None => [].iter(),
        }
    }

    pub fn version_string(&self) -> &str {
        self.bundle_settings.version.as_ref().unwrap_or(&self.cargo_settings.package.version)
    }

    pub fn copyright_string(&self) -> Option<&str> {
        self.bundle_settings.copyright.as_ref().map(String::as_str)
    }

    pub fn author_names(&self) -> std::slice::Iter<String> {
        match self.cargo_settings.package.authors {
            Some(ref names) => names.iter(),
            None => [].iter(),
        }
    }

    pub fn homepage_url(&self) -> &str {
        &self.cargo_settings.package.homepage.as_ref().map(String::as_str).unwrap_or("")
    }

    pub fn short_description(&self) -> &str {
        self.bundle_settings.short_description.as_ref().unwrap_or(&self.cargo_settings.package.description)
    }

    pub fn long_description(&self) -> Option<&str> {
        self.bundle_settings.long_description.as_ref().map(String::as_str)
    }
}

const BUNDLE_TOML_WARNING: &str = "\
Using Bundle.toml file, which is deprecated in favor
  of using [package.metadata.bundle] section in Cargo.toml
  file.  Support for Bundle.toml file will be removed in a
  future version of cargo-bundle.";
