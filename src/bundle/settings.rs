use super::category::AppCategory;
use super::common::print_warning;
use super::localization::{LinuxDesktopLocale, LinuxDesktopLocalizations, OsxLocalizations};
use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use target_build_utils::TargetInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageType {
    OsxBundle,
    OsxDmg,
    IosBundle,
    WindowsMsi,
    WxsMsi,
    WindowsBundle,
    Deb,
    Rpm,
    AppImage,
}

impl std::str::FromStr for PackageType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PackageType::try_from(s)
    }
}

impl std::fmt::Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

impl TryFrom<&str> for PackageType {
    type Error = anyhow::Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        PackageType::from_short_name(s).ok_or_else(|| {
            let all = PackageType::all()
                .iter()
                .map(|&s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::anyhow!("Unsupported package type: '{s}'. Supported types are: {all}")
        })
    }
}

impl TryFrom<String> for PackageType {
    type Error = anyhow::Error;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        PackageType::try_from(s.as_str())
    }
}

impl PackageType {
    pub fn from_short_name(name: &str) -> Option<PackageType> {
        // Other types we may eventually want to support: apk
        match name {
            "deb" => Some(PackageType::Deb),
            "ios" => Some(PackageType::IosBundle),
            "msi" => Some(PackageType::WindowsMsi),
            "wxsmsi" => Some(PackageType::WxsMsi),
            "osx" => Some(PackageType::OsxBundle),
            "dmg" => Some(PackageType::OsxDmg),
            "rpm" => Some(PackageType::Rpm),
            "appimage" => Some(PackageType::AppImage),
            "exe" => Some(PackageType::WindowsBundle),
            _ => None,
        }
    }

    pub const fn short_name(&self) -> &'static str {
        match *self {
            PackageType::Deb => "deb",
            PackageType::IosBundle => "ios",
            PackageType::WindowsMsi => "msi",
            PackageType::WxsMsi => "wxsmsi",
            PackageType::OsxBundle => "osx",
            PackageType::OsxDmg => "dmg",
            PackageType::Rpm => "rpm",
            PackageType::AppImage => "appimage",
            PackageType::WindowsBundle => "exe",
        }
    }

    pub const fn all() -> &'static [&'static str] {
        &[
            "deb", "ios", "msi", "wxsmsi", "osx", "dmg", "rpm", "appimage", "exe",
        ]
    }
}

#[derive(Clone, Debug)]
pub enum BuildArtifact {
    Main,
    Bin(String),
    Example(String),
}

/// A single FreeDesktop desktop action.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct DesktopAction {
    /// Localizable display name for the action (required by the spec).
    #[serde(rename = "Name")]
    pub name: String,

    /// Command to execute; falls back to the app binary when absent.
    #[serde(rename = "Exec", default)]
    pub exec: Option<String>,

    /// Icon for this action.
    #[serde(rename = "Icon", default)]
    pub icon: Option<String>,

    /// Per-locale names.
    #[serde(rename = "NameLocalized", default)]
    pub name_localized: Option<HashMap<String, String>>, // local code to translation
}

/// Windows Authenticode signing configuration. This is available only with
/// cargo-bundle's `windows-signing` feature because its implementation links
/// GPL-3.0-or-later code.
#[derive(Clone, Debug, serde::Deserialize)]
#[cfg_attr(not(feature = "windows-signing"), allow(dead_code))]
pub struct WindowsSigningSettings {
    /// Path to a PKCS#12 (`.p12`/`.pfx`) signing certificate.
    pub certificate_path: PathBuf,
    /// Name of the environment variable containing the certificate password.
    #[serde(default)]
    pub certificate_password_env: Option<String>,
    /// Optional RFC 3161 timestamp service URL.
    #[serde(default)]
    pub timestamp_url: Option<String>,
}

/// Keyless Sigstore configuration for Linux release artifacts.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct LinuxSigningSettings {
    /// Environment variable containing an OIDC token minted for the
    /// `sigstore` audience.
    pub identity_token_env: String,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LinuxSettings {
    mime_types: Option<Vec<String>>,
    exec_args: Option<String>,
    use_terminal: Option<bool>,
    localizations: Option<HashMap<String, LinuxDesktopLocale>>,
    startup_wm_class: Option<String>,
    desktop_actions: Option<HashMap<String, DesktopAction>>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OsxSettings {
    frameworks: Option<Vec<String>>,
    plugins: Option<Vec<String>>,
    minimum_system_version: Option<String>,
    url_schemes: Option<Vec<String>>,
    info_plist_exts: Option<Vec<String>>,
    localizations: Option<HashMap<String, HashMap<String, String>>>,
    dmg_background: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSettings {
    // General settings:
    name: Option<String>,
    identifier: Option<String>,
    icon: Option<Vec<String>>,
    version: Option<String>,
    resources: Option<Vec<String>>,
    copyright: Option<String>,
    category: Option<AppCategory>,
    short_description: Option<String>,
    long_description: Option<String>,
    /// Path to an AppStream metainfo XML file to bundle in the AppImage.
    appimage_metainfo_path: Option<String>,
    /// SquashFS compression codec: `"gzip"` (default), `"lz4"`, `"lzo"`, or `"none"`.
    appimage_compression: Option<String>,
    deb_depends: Option<Vec<String>>,
    /// Linux-only packaging configuration.
    linux: Option<LinuxSettings>,
    /// macOS-only packaging configuration.
    #[serde(alias = "macos")]
    osx: Option<OsxSettings>,
    /// PKCS#12 certificate used by the pure-Rust Apple signing backend.
    /// The bundle is left unsigned when this is not configured.
    apple_signing_p12: Option<PathBuf>,
    /// Environment variable containing the PKCS#12 certificate password.
    apple_signing_password_env: Option<String>,
    /// Optional RFC 3161 timestamp service URL for Apple code signatures.
    apple_signing_timestamp_url: Option<String>,
    /// Optional entitlements plist embedded in Apple code signatures.
    apple_signing_entitlements: Option<PathBuf>,
    /// Enable the hardened runtime in Apple code signatures.
    apple_signing_hardened_runtime: Option<bool>,
    /// Optional Authenticode configuration for `.exe` and `.msi` output.
    windows_signing: Option<WindowsSigningSettings>,
    /// Optional keyless Sigstore configuration for Linux release artifacts.
    linux_signing: Option<LinuxSigningSettings>,
    // Bundles for other binaries/examples:
    bin: Option<HashMap<String, BundleSettings>>,
    example: Option<HashMap<String, BundleSettings>>,
}

#[derive(Clone, Debug)]
pub struct Settings {
    package: cargo_metadata::Package,
    package_type: Option<PackageType>, // If `None`, use the default package type for this os
    /// Explicit target triples; empty means "build for the host". More than
    /// one produces a universal binary combined with `lipo` (macOS only).
    targets: Vec<(String, TargetInfo)>,
    features: Option<String>,
    project_out_directory: PathBuf,
    build_artifact: BuildArtifact,
    profile: String,
    all_features: bool,
    no_default_features: bool,
    prebuilt_binary: bool,
    binary_path: PathBuf,
    /// Per-target binaries that `lipo` combines into `binary_path` when more
    /// than one target triple was requested; empty otherwise.
    universal_input_binary_paths: Vec<PathBuf>,
    binary_name: String,
    bundle_settings: BundleSettings,
}

/// Try to load `Cargo.toml` file in the specified directory
fn load_metadata(dir: &Path) -> crate::Result<Metadata> {
    let cargo_file_path = dir.join("Cargo.toml");
    Ok(MetadataCommand::new()
        .manifest_path(cargo_file_path)
        .exec()?)
}

impl Settings {
    pub fn new(current_dir: PathBuf, cli: &crate::Cli) -> crate::Result<Self> {
        let package_type = cli.format;
        let build_artifact = if let Some(bin) = cli.bin.as_ref() {
            BuildArtifact::Bin(bin.to_string())
        } else if let Some(example) = cli.example.as_ref() {
            BuildArtifact::Example(example.to_string())
        } else {
            BuildArtifact::Main
        };
        let profile = if cli.release {
            "release".to_string()
        } else if let Some(profile) = cli.profile.as_ref() {
            if profile == "debug" {
                anyhow::bail!("Profile name `debug` is reserved")
            }
            profile.to_string()
        } else {
            "dev".to_string()
        };
        let all_features = cli.all_features;
        let no_default_features = cli.no_default_features;
        let targets = cli
            .target
            .iter()
            .map(|triple| Ok((triple.to_string(), TargetInfo::from_str(triple)?)))
            .collect::<crate::Result<Vec<_>>>()?;
        let features = cli.features.as_ref().map(|features| features.into());
        let cargo_settings = load_metadata(&current_dir)?;
        let package = Settings::find_bundle_package(cli.package.as_deref(), &cargo_settings)?;
        let bundle_settings = Settings::bundle_settings_of_package(package)?;
        let workspace_dir = Settings::get_workspace_dir(current_dir.clone());
        // With multiple targets the per-target binaries are combined into a
        // universal binary living under its own `universal` directory.
        let target_dir_name = match targets.as_slice() {
            [] => None,
            [(triple, _)] => Some(triple.as_str()),
            _ => Some("universal"),
        };
        let target_dir =
            Settings::get_target_dir(&workspace_dir, target_dir_name, &profile, &build_artifact);
        let (bundle_settings, mut binary_name) = match &build_artifact {
            BuildArtifact::Main => {
                if let Some(target) = package
                    .targets
                    .iter()
                    .find(|target| target.kind.contains(&TargetKind::Bin))
                {
                    (bundle_settings, target.name.clone())
                } else {
                    anyhow::bail!("No `bin` target is found in package '{}'", package.name)
                }
            }
            BuildArtifact::Bin(name) => (
                bundle_settings_from_table(&bundle_settings.bin, "bin", name)?,
                name.clone(),
            ),
            BuildArtifact::Example(name) => (
                bundle_settings_from_table(&bundle_settings.example, "example", name)?,
                name.clone(),
            ),
        };
        let binary_extension = match package_type {
            Some(PackageType::WindowsMsi)
            | Some(PackageType::WxsMsi)
            | Some(PackageType::WindowsBundle) => ".exe",
            _ => "",
        };
        binary_name += binary_extension;
        let prebuilt_binary = cli.binary_path.is_some();
        let binary_path = if let Some(path) = &cli.binary_path {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                current_dir.join(path)
            };
            let metadata = std::fs::metadata(&path).map_err(|error| {
                anyhow::anyhow!("Failed to read prebuilt binary {path:?}: {error}")
            })?;
            if !metadata.is_file() {
                anyhow::bail!("Prebuilt binary path is not a file: {path:?}");
            }
            path
        } else {
            target_dir.join(&binary_name)
        };
        let universal_input_binary_paths = if !prebuilt_binary && targets.len() > 1 {
            targets
                .iter()
                .map(|(triple, _)| {
                    Settings::get_target_dir(
                        &workspace_dir,
                        Some(triple),
                        &profile,
                        &build_artifact,
                    )
                    .join(&binary_name)
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Settings {
            package: package.clone(),
            package_type,
            targets,
            features,
            build_artifact,
            profile,
            all_features,
            no_default_features,
            prebuilt_binary,
            project_out_directory: target_dir,
            binary_path,
            universal_input_binary_paths,
            binary_name,
            bundle_settings,
        })
    }

    pub fn manifest_path(&self) -> &Path {
        Path::new(&self.package.manifest_path)
    }

    /*
        The target_dir where binaries will be compiled to by cargo can vary:
            - this directory is a member of a workspace project
            - overridden by CARGO_TARGET_DIR environment variable
            - specified in build.target-dir configuration key
            - if the build is a 'release' or 'debug' build

        This function determines where 'target' dir is and suffixes it with 'release' or 'debug'
        to determine where the compiled binary will be located.
    */
    fn get_target_dir(
        project_root_dir: &Path,
        target_dir_name: Option<&str>,
        profile: &str,
        build_artifact: &BuildArtifact,
    ) -> PathBuf {
        let mut cargo = std::process::Command::new(
            std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")),
        );
        cargo.args(["metadata", "--no-deps", "--format-version", "1"]);

        let target_dir = cargo.output().ok().and_then(|output| {
            let json_string = String::from_utf8(output.stdout).ok()?;
            let json: Value = serde_json::from_str(&json_string).ok()?;
            Some(PathBuf::from(json.get("target_directory")?.as_str()?))
        });

        let mut path = target_dir.unwrap_or(project_root_dir.join("target"));

        if let Some(name) = target_dir_name {
            path.push(name);
        }
        path.push(if profile == "dev" { "debug" } else { profile });
        if let &BuildArtifact::Example(_) = build_artifact {
            path.push("examples");
        }
        path
    }

    /*
        The specification of the Cargo.toml Manifest that covers the "workspace" section is here:
        https://doc.rust-lang.org/cargo/reference/manifest.html#the-workspace-section

        Determining if the current project folder is part of a workspace:
            - Walk up the file system, looking for a Cargo.toml file.
            - Stop at the first one found.
            - If one is found before reaching "/" then this folder belongs to that parent workspace
    */
    fn get_workspace_dir(current_dir: PathBuf) -> PathBuf {
        let mut dir = current_dir.clone();
        let set = load_metadata(&dir);
        if set.is_ok() {
            return dir;
        }
        while dir.pop() {
            let set = load_metadata(&dir);
            if set.is_ok() {
                return dir;
            }
        }

        // Nothing found walking up the file system, return the starting directory
        current_dir
    }

    fn find_bundle_package<'a>(
        package: Option<&'a str>,
        metadata: &'a Metadata,
    ) -> crate::Result<&'a Package> {
        match package {
            Some(package) => metadata
                .packages
                .iter()
                .find(|p| p.name.as_str() == package)
                .ok_or_else(|| anyhow::anyhow!("Package '{package}' not found in workspace")),
            None => metadata
                .root_package()
                .ok_or_else(|| anyhow::anyhow!("No root package found in workspace")),
        }
    }

    fn bundle_settings_of_package(package: &Package) -> crate::Result<BundleSettings> {
        if let Some(bundle) = package.metadata.get("bundle") {
            return Ok(serde_json::from_value::<BundleSettings>(bundle.clone())?);
        }
        print_warning(&format!(
            "No [package.metadata.bundle] section in package \"{}\"",
            package.name
        ))?;
        Ok(BundleSettings::default())
    }

    /// Returns the directory where the bundle should be placed.
    pub fn project_out_directory(&self) -> &Path {
        &self.project_out_directory
    }

    /// Returns the architecture for the binary being bundled (e.g. "arm" or
    /// "x86" or "x86_64").
    pub fn binary_arch(&self) -> &str {
        if let Some((_, info)) = self.targets.first() {
            info.target_arch()
        } else {
            std::env::consts::ARCH
        }
    }

    /// Returns the file name of the binary being bundled.
    pub fn binary_name(&self) -> &str {
        &self.binary_name
    }

    /// Returns the path to the binary being bundled.
    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }

    /// Whether the executable was supplied with `--binary-path` rather than
    /// being produced by cargo-bundle's own `cargo build` invocation.
    pub fn uses_prebuilt_binary(&self) -> bool {
        self.prebuilt_binary
    }

    /// If a specific package type was specified by the command-line, returns
    /// that package type; otherwise, if a target triple was specified by the
    /// command-line, returns the native package type(s) for that target;
    /// otherwise, returns the native package type(s) for the host platform.
    /// Fails if the host/target's native package type is not supported.
    pub fn package_types(&self) -> crate::Result<Vec<PackageType>> {
        if let Some(package_type) = self.package_type {
            Ok(vec![package_type])
        } else {
            let target_os = if let Some((_, info)) = self.targets.first() {
                info.target_os()
            } else {
                std::env::consts::OS
            };
            match target_os {
                "macos" => Ok(vec![PackageType::OsxBundle, PackageType::OsxDmg]),
                "ios" => Ok(vec![PackageType::IosBundle]),
                "linux" => Ok(vec![PackageType::Deb, PackageType::AppImage]), // TODO: Do Rpm too, once it's implemented.
                "windows" => Ok(vec![PackageType::WindowsMsi, PackageType::WindowsBundle]),
                os => anyhow::bail!("Native {} bundles not yet supported.", os),
            }
        }
    }

    /// Every target triple requested on the command line; empty when
    /// building for the host.
    pub fn target_triples(&self) -> impl Iterator<Item = &str> {
        self.targets.iter().map(|(triple, _)| triple.as_str())
    }

    /// The per-target binaries that must be combined with `lipo` into
    /// `binary_path`; empty unless more than one target was requested.
    pub fn universal_input_binary_paths(&self) -> &[PathBuf] {
        &self.universal_input_binary_paths
    }

    pub fn features(&self) -> Option<&str> {
        match self.features {
            Some(ref features) => Some(features.as_str()),
            None => None,
        }
    }

    /// Returns the artifact that is being bundled.
    pub fn build_artifact(&self) -> &BuildArtifact {
        &self.build_artifact
    }

    /// Returns `release`, 'dev` or other profile.
    pub fn build_profile(&self) -> &str {
        &self.profile
    }

    pub fn all_features(&self) -> bool {
        self.all_features
    }

    pub fn no_default_features(&self) -> bool {
        self.no_default_features
    }

    pub fn bundle_name(&self) -> &str {
        self.bundle_settings
            .name
            .as_ref()
            .unwrap_or(&self.package.name)
    }

    pub fn bundle_identifier(&self) -> Cow<'_, str> {
        if let Some(identifier) = &self.bundle_settings.identifier {
            identifier.into()
        } else {
            match &self.build_artifact {
                BuildArtifact::Main => "".into(),
                BuildArtifact::Bin(name) => format!("{name}.{}", self.package.name).into(),
                BuildArtifact::Example(name) => {
                    format!("{name}.example.{}", self.package.name).into()
                }
            }
        }
    }

    /// Returns an iterator over the icon files to be used for this bundle.
    pub fn icon_files(&self) -> ResourcePaths<'_> {
        match self.bundle_settings.icon {
            Some(ref paths) => ResourcePaths::new(paths.as_slice(), false),
            None => ResourcePaths::new(&[], false),
        }
    }

    /// Returns an iterator over the resource files to be included in this
    /// bundle.
    pub fn resource_files(&self) -> ResourcePaths<'_> {
        match self.bundle_settings.resources {
            Some(ref paths) => ResourcePaths::new(paths.as_slice(), true),
            None => ResourcePaths::new(&[], true),
        }
    }

    pub fn version_string(&self) -> &dyn Display {
        match self.bundle_settings.version.as_ref() {
            Some(v) => v,
            None => &self.package.version,
        }
    }

    pub fn copyright_string(&self) -> Option<&str> {
        self.bundle_settings.copyright.as_deref()
    }

    pub fn author_names(&self) -> &[String] {
        &self.package.authors
    }

    pub fn authors_comma_separated(&self) -> Option<String> {
        let names = self.author_names();
        if names.is_empty() {
            None
        } else {
            Some(names.join(", "))
        }
    }

    pub fn homepage_url(&self) -> &str {
        self.package.homepage.as_deref().unwrap_or("")
    }

    pub fn app_category(&self) -> Option<AppCategory> {
        self.bundle_settings.category
    }

    pub fn short_description(&self) -> &str {
        self.bundle_settings
            .short_description
            .as_deref()
            .unwrap_or_else(|| self.package.description.as_deref().unwrap_or(""))
    }

    pub fn long_description(&self) -> Option<&str> {
        self.bundle_settings.long_description.as_deref()
    }

    pub fn license_content(&self) -> Option<String> {
        self.package
            .license_file
            .as_ref()
            .and_then(|license_file| {
                let dir = self
                    .manifest_path()
                    .parent()
                    .unwrap_or_else(|| Path::new("."));

                let license_path = dir.join(license_file);
                match std::fs::read_to_string(&license_path) {
                    Ok(content) => Some(content),
                    Err(err) => {
                        print_warning(&format!(
                            "Failed to read license file '{license_path:?}': {err} -- ignoring",
                        ))
                        .ok();
                        None
                    }
                }
            })
            .or_else(|| self.package.license.as_ref().map(|s| s.to_string()))
    }

    pub fn debian_dependencies(&self) -> &[String] {
        match self.bundle_settings.deb_depends {
            Some(ref dependencies) => dependencies.as_slice(),
            None => &[],
        }
    }

    pub fn linux_mime_types(&self) -> &[String] {
        self.bundle_settings
            .linux
            .as_ref()
            .and_then(|linux| linux.mime_types.as_deref())
            .unwrap_or_default()
    }

    pub fn linux_use_terminal(&self) -> Option<bool> {
        self.bundle_settings
            .linux
            .as_ref()
            .and_then(|linux| linux.use_terminal)
    }

    pub fn linux_exec_args(&self) -> Option<&str> {
        self.bundle_settings
            .linux
            .as_ref()
            .and_then(|linux| linux.exec_args.as_deref())
    }

    /// Path to an AppStream metainfo XML to bundle in the AppImage.
    pub fn appimage_metainfo_path(&self) -> Option<&str> {
        self.bundle_settings.appimage_metainfo_path.as_deref()
    }

    /// SquashFS compression codec: `"gzip"` (default), `"lz4"`, `"lzo"`, or `"none"`.
    pub fn appimage_compression(&self) -> Option<&str> {
        self.bundle_settings.appimage_compression.as_deref()
    }

    /// `StartupWMClass` for the `.desktop` entry.
    pub fn linux_startup_wm_class(&self) -> Option<&str> {
        self.bundle_settings
            .linux
            .as_ref()
            .and_then(|linux| linux.startup_wm_class.as_deref())
    }

    /// Desktop actions to emit as `[Desktop Action <id>]` groups.
    pub fn linux_desktop_actions(&self) -> Option<&HashMap<String, DesktopAction>> {
        self.bundle_settings
            .linux
            .as_ref()
            .and_then(|linux| linux.desktop_actions.as_ref())
    }

    pub fn osx_frameworks(&self) -> &[String] {
        self.bundle_settings
            .osx
            .as_ref()
            .and_then(|osx| osx.frameworks.as_deref())
            .unwrap_or_default()
    }

    pub fn osx_plugins(&self) -> &[String] {
        self.bundle_settings
            .osx
            .as_ref()
            .and_then(|osx| osx.plugins.as_deref())
            .unwrap_or_default()
    }

    pub fn osx_minimum_system_version(&self) -> Option<&str> {
        self.bundle_settings
            .osx
            .as_ref()
            .and_then(|osx| osx.minimum_system_version.as_deref())
    }

    pub fn osx_url_schemes(&self) -> &[String] {
        self.bundle_settings
            .osx
            .as_ref()
            .and_then(|osx| osx.url_schemes.as_deref())
            .unwrap_or_default()
    }

    /// Returns an iterator over the plist files for this bundle
    pub fn osx_info_plist_exts(&self) -> ResourcePaths<'_> {
        ResourcePaths::new(
            self.bundle_settings
                .osx
                .as_ref()
                .and_then(|osx| osx.info_plist_exts.as_deref())
                .unwrap_or_default(),
            false,
        )
    }

    /// macOS localizations as an [`OsxLocalizations`] wrapper.
    ///
    /// Writes `*.lproj/InfoPlist.strings` under the Resources directory.
    pub fn osx_localizations(&self) -> Option<OsxLocalizations<'_>> {
        self.bundle_settings
            .osx
            .as_ref()
            .and_then(|osx| osx.localizations.as_ref())
            .map(OsxLocalizations::new)
    }

    /// Linux desktop localizations as a [`LinuxDesktopLocalizations`] wrapper.
    ///
    /// Keys are inlined into the `.desktop` file as `Name[locale]=…` etc.
    /// Unlocalized `Name` / `Comment` still come from
    /// [`bundle_name`](Self::bundle_name) / [`short_description`](Self::short_description).
    pub fn linux_localizations(&self) -> Option<LinuxDesktopLocalizations<'_>> {
        self.bundle_settings
            .linux
            .as_ref()
            .and_then(|linux| linux.localizations.as_ref())
            .map(LinuxDesktopLocalizations::new)
    }

    /// An SVG that becomes the Finder window background of the DMG. It must
    /// contain elements with ids `app` and `applications`, whose centers
    /// become the icon positions.
    pub fn osx_dmg_background(&self) -> Option<&Path> {
        self.bundle_settings
            .osx
            .as_ref()
            .and_then(|osx| osx.dmg_background.as_deref())
    }

    /// PKCS#12 certificate used by the pure-Rust Apple signing backend.
    pub fn apple_signing_p12(&self) -> Option<&Path> {
        self.bundle_settings.apple_signing_p12.as_deref()
    }

    /// Environment variable containing the Apple PKCS#12 certificate password.
    pub fn apple_signing_password_env(&self) -> Option<&str> {
        self.bundle_settings.apple_signing_password_env.as_deref()
    }

    /// Optional RFC 3161 timestamp service URL for Apple code signatures.
    pub fn apple_signing_timestamp_url(&self) -> Option<&str> {
        self.bundle_settings.apple_signing_timestamp_url.as_deref()
    }

    /// Entitlements plist embedded in Apple code signatures.
    pub fn apple_signing_entitlements(&self) -> Option<&Path> {
        self.bundle_settings.apple_signing_entitlements.as_deref()
    }

    /// Whether to enable the hardened runtime during Apple code signing.
    pub fn apple_signing_hardened_runtime(&self) -> bool {
        self.bundle_settings
            .apple_signing_hardened_runtime
            .unwrap_or(false)
    }

    /// Authenticode configuration, when Windows signing has been requested.
    pub fn windows_signing(&self) -> Option<&WindowsSigningSettings> {
        self.bundle_settings.windows_signing.as_ref()
    }

    /// Keyless Sigstore configuration for Linux release artifacts.
    pub fn linux_signing(&self) -> Option<&LinuxSigningSettings> {
        self.bundle_settings.linux_signing.as_ref()
    }
}

fn bundle_settings_from_table(
    opt_map: &Option<HashMap<String, BundleSettings>>,
    map_name: &str,
    bundle_name: &str,
) -> crate::Result<BundleSettings> {
    if let Some(bundle_settings) = opt_map.as_ref().and_then(|map| map.get(bundle_name)) {
        Ok(bundle_settings.clone())
    } else {
        print_warning(&format!(
            "No [package.metadata.bundle.{map_name}.{bundle_name}] section in Cargo.toml"
        ))?;
        Ok(BundleSettings::default())
    }
}

pub struct ResourcePaths<'a> {
    pattern_iter: std::slice::Iter<'a, String>,
    glob_iter: Option<glob::Paths>,
    walk_iter: Option<walkdir::IntoIter>,
    allow_walk: bool,
}

impl<'a> ResourcePaths<'a> {
    fn new(patterns: &'a [String], allow_walk: bool) -> ResourcePaths<'a> {
        ResourcePaths {
            pattern_iter: patterns.iter(),
            glob_iter: None,
            walk_iter: None,
            allow_walk,
        }
    }
}

impl Iterator for ResourcePaths<'_> {
    type Item = crate::Result<PathBuf>;

    fn next(&mut self) -> Option<crate::Result<PathBuf>> {
        loop {
            if let Some(ref mut walk_entries) = self.walk_iter
                && let Some(entry) = walk_entries.next()
            {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => return Some(Err(anyhow::Error::from(error))),
                };
                let path = entry.path();
                if path.is_dir() {
                    continue;
                }
                return Some(Ok(path.to_path_buf()));
            }
            self.walk_iter = None;
            if let Some(ref mut glob_paths) = self.glob_iter
                && let Some(glob_result) = glob_paths.next()
            {
                let path = match glob_result {
                    Ok(path) => path,
                    Err(error) => return Some(Err(anyhow::Error::from(error))),
                };
                if path.is_dir() {
                    if self.allow_walk {
                        let walk = walkdir::WalkDir::new(path);
                        self.walk_iter = Some(walk.into_iter());
                        continue;
                    } else {
                        return Some(Err(anyhow::anyhow!("{path:?} is a directory")));
                    }
                }
                return Some(Ok(path));
            }
            self.glob_iter = None;
            if let Some(pattern) = self.pattern_iter.next() {
                let glob = match glob::glob(pattern) {
                    Ok(glob) => glob,
                    Err(error) => return Some(Err(anyhow::Error::from(error))),
                };
                self.glob_iter = Some(glob);
                continue;
            }
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppCategory, BundleSettings};
    use crate::bundle::localization::DesktopKeywords;
    use std::path::PathBuf;

    #[test]
    fn parse_cargo_toml() {
        let toml_str = "\
            name = \"Example Application\"\n\
            identifier = \"com.example.app\"\n\
            resources = [\"data\", \"foo/bar\"]\n\
            category = \"Puzzle Game\"\n\
            long_description = \"\"\"\n\
            This is an example of a\n\
            simple application.\n\
            \"\"\"\n";
        let bundle: BundleSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(bundle.name, Some("Example Application".to_string()));
        assert_eq!(bundle.identifier, Some("com.example.app".to_string()));
        assert_eq!(bundle.icon, None);
        assert_eq!(bundle.version, None);
        assert_eq!(
            bundle.resources,
            Some(vec!["data".to_string(), "foo/bar".to_string()])
        );
        assert_eq!(bundle.category, Some(AppCategory::PuzzleGame));
        assert_eq!(
            bundle.long_description,
            Some(
                "This is an example of a\n\
                         simple application.\n"
                    .to_string()
            )
        );
    }

    #[test]
    fn platform_tables_are_typed() {
        let bundle: BundleSettings = toml::from_str(
            r#"
                name = "Cross-platform name"
                identifier = "com.example.shared"
                resources = ["shared"]

                [osx]
                frameworks = ["WebKit.framework"]
            "#,
        )
        .unwrap();

        assert_eq!(bundle.name.as_deref(), Some("Cross-platform name"));
        assert_eq!(
            bundle.osx.unwrap().frameworks,
            Some(vec!["WebKit.framework".into()])
        );
        assert!(toml::from_str::<BundleSettings>("frameworks = [\"WebKit.framework\"]").is_err());
        assert!(
            toml::from_str::<BundleSettings>("osx_frameworks = [\"WebKit.framework\"]").is_err()
        );
    }

    #[test]
    fn parses_signing_configuration() {
        let bundle: BundleSettings = toml::from_str(
            r#"
                apple_signing_p12 = "certs/apple.p12"
                apple_signing_password_env = "APPLE_SIGNING_PASSWORD"

                [windows_signing]
                certificate_path = "certs/windows.p12"
                certificate_password_env = "WINDOWS_SIGNING_PASSWORD"

                [linux_signing]
                identity_token_env = "SIGSTORE_ID_TOKEN"
            "#,
        )
        .unwrap();

        assert_eq!(
            bundle.apple_signing_p12,
            Some(PathBuf::from("certs/apple.p12"))
        );
        assert_eq!(
            bundle
                .windows_signing
                .as_ref()
                .unwrap()
                .certificate_password_env
                .as_deref(),
            Some("WINDOWS_SIGNING_PASSWORD")
        );
        assert_eq!(
            bundle.linux_signing.as_ref().unwrap().identity_token_env,
            "SIGSTORE_ID_TOKEN"
        );
    }

    #[test]
    fn parse_bin_and_example_bundles() {
        let toml_str = "\
            [bin.foo]\n\
            name = \"Foo App\"\n\
            \n\
            [bin.bar]\n\
            name = \"Bar App\"\n\
            \n\
            [example.baz]\n\
            name = \"Baz Example\"\n";
        let bundle: BundleSettings = toml::from_str(toml_str).unwrap();
        assert!(bundle.example.is_some());

        let bins = bundle.bin.as_ref().unwrap();
        assert!(bins.contains_key("foo"));
        let foo: &BundleSettings = bins.get("foo").unwrap();
        assert_eq!(foo.name, Some("Foo App".to_string()));
        assert!(bins.contains_key("bar"));
        let bar: &BundleSettings = bins.get("bar").unwrap();
        assert_eq!(bar.name, Some("Bar App".to_string()));

        let examples = bundle.example.as_ref().unwrap();
        assert!(examples.contains_key("baz"));
        let baz: &BundleSettings = examples.get("baz").unwrap();
        assert_eq!(baz.name, Some("Baz Example".to_string()));
    }

    #[test]
    fn dmg_round_trip() {
        use super::PackageType;

        assert_eq!(
            PackageType::from_short_name("dmg"),
            Some(PackageType::OsxDmg)
        );

        assert_eq!(PackageType::OsxDmg.short_name(), "dmg");
        assert_eq!(PackageType::OsxDmg.to_string(), "dmg");
    }

    #[test]
    fn exe_round_trip() {
        use super::PackageType;

        assert_eq!(
            PackageType::from_short_name("exe"),
            Some(PackageType::WindowsBundle)
        );

        assert_eq!(PackageType::WindowsBundle.short_name(), "exe");
        assert_eq!(PackageType::WindowsBundle.to_string(), "exe");
    }

    #[test]
    fn all_package_types_are_listed() {
        use super::PackageType;
        let all = PackageType::all();
        assert!(all.contains(&"dmg"), "dmg missing from PackageType::all()");
        assert!(all.contains(&"exe"), "exe missing from PackageType::all()");
    }

    #[test]
    fn osx_localizations_parses_from_toml() {
        let toml_str = r#"
            [osx.localizations.fr]
            CFBundleDisplayName = "Mon Application"

            [osx.localizations.de]
            CFBundleDisplayName = "Meine Anwendung"
        "#;
        let bundle: BundleSettings = toml::from_str(toml_str).unwrap();
        let locs = bundle.osx.unwrap().localizations.unwrap();
        assert_eq!(locs["fr"]["CFBundleDisplayName"], "Mon Application");
        assert_eq!(locs["de"]["CFBundleDisplayName"], "Meine Anwendung");
    }

    #[test]
    fn linux_localizations_parses_from_toml() {
        let toml_str = r#"
            [linux.localizations.fr]
            Name = "Mon App"
            Comment = "Une description"
            GenericName = "Utilitaire"
            Keywords = ["outil", "utilitaire"]

            [linux.localizations.de]
            Name = "Meine App"
            Comment = "Eine Beschreibung"
            Keywords = "werkzeug;dienstprogramm"

            [linux.localizations.pt_BR]
            Name = "Meu App"
            Comment = "Uma descrição"
        "#;
        let bundle: BundleSettings = toml::from_str(toml_str).unwrap();
        let locs = bundle.linux.unwrap().localizations.unwrap();

        assert_eq!(locs["fr"].name.as_deref(), Some("Mon App"));
        assert_eq!(locs["fr"].comment.as_deref(), Some("Une description"));
        assert_eq!(locs["fr"].generic_name.as_deref(), Some("Utilitaire"));
        match locs["fr"].keywords.as_ref().unwrap() {
            DesktopKeywords::List(items) => {
                assert_eq!(items, &["outil".to_string(), "utilitaire".to_string()]);
            }
            DesktopKeywords::String(_) => panic!("expected Keywords list for fr"),
        }

        assert_eq!(locs["de"].name.as_deref(), Some("Meine App"));
        match locs["de"].keywords.as_ref().unwrap() {
            DesktopKeywords::String(s) => assert_eq!(s, "werkzeug;dienstprogramm"),
            DesktopKeywords::List(_) => panic!("expected Keywords string for de"),
        }

        assert_eq!(locs["pt_BR"].name.as_deref(), Some("Meu App"));
        assert!(locs["pt_BR"].generic_name.is_none());
        assert!(locs["pt_BR"].keywords.is_none());
    }

    #[test]
    fn parse_appimage_feature_settings() {
        let toml_str = r#"
            name = "AppImage App"
            identifier = "com.example.appimage"
            appimage_metainfo_path = "assets/metainfo.xml"
            appimage_compression = "lz4"

            [linux]
            startup_wm_class = "myapp"

            [linux.desktop_actions.new-window]
            Name = "New Window"
            Exec = "myapp --new-window"

            [linux.desktop_actions.new-window.NameLocalized]
            fr = "Nouvelle fenetre"
        "#;
        let bundle: BundleSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(
            bundle.appimage_metainfo_path.as_deref(),
            Some("assets/metainfo.xml")
        );
        assert_eq!(bundle.appimage_compression.as_deref(), Some("lz4"));
        assert_eq!(
            bundle.linux.as_ref().unwrap().startup_wm_class.as_deref(),
            Some("myapp")
        );

        let actions = bundle.linux.unwrap().desktop_actions.unwrap();
        let action = &actions["new-window"];
        assert_eq!(action.name, "New Window");
        assert_eq!(action.exec.as_deref(), Some("myapp --new-window"));
        assert!(action.icon.is_none());
        assert_eq!(
            action.name_localized.as_ref().unwrap()["fr"],
            "Nouvelle fenetre"
        );
    }
}
