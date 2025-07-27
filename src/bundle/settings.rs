use super::category::AppCategory;
use super::common::print_warning;
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
    IosBundle,
    WindowsMsi,
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
            "osx" => Some(PackageType::OsxBundle),
            "rpm" => Some(PackageType::Rpm),
            "appimage" => Some(PackageType::AppImage),
            _ => None,
        }
    }

    pub const fn short_name(&self) -> &'static str {
        match *self {
            PackageType::Deb => "deb",
            PackageType::IosBundle => "ios",
            PackageType::WindowsMsi => "msi",
            PackageType::OsxBundle => "osx",
            PackageType::Rpm => "rpm",
            PackageType::AppImage => "appimage",
        }
    }

    pub const fn all() -> &'static [&'static str] {
        &["deb", "ios", "msi", "osx", "rpm", "appimage"]
    }
}

#[derive(Clone, Debug)]
pub enum BuildArtifact {
    Main,
    Bin(String),
    Example(String),
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
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
    // OS-specific settings:
    linux_mime_types: Option<Vec<String>>,
    linux_exec_args: Option<String>,
    linux_use_terminal: Option<bool>,
    deb_depends: Option<Vec<String>>,
    osx_frameworks: Option<Vec<String>>,
    osx_plugins: Option<Vec<String>>,
    osx_minimum_system_version: Option<String>,
    osx_url_schemes: Option<Vec<String>>,
    osx_info_plist_exts: Option<Vec<String>>,
    // Bundles for other binaries/examples:
    bin: Option<HashMap<String, BundleSettings>>,
    example: Option<HashMap<String, BundleSettings>>,
}

#[derive(Clone, Debug)]
pub struct Settings {
    package: cargo_metadata::Package,
    package_type: Option<PackageType>, // If `None`, use the default package type for this os
    target: Option<(String, TargetInfo)>,
    features: Option<String>,
    project_out_directory: PathBuf,
    build_artifact: BuildArtifact,
    profile: String,
    all_features: bool,
    no_default_features: bool,
    binary_path: PathBuf,
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
        let target = if let Some(triple) = cli.target.as_ref() {
            Some((triple.to_string(), TargetInfo::from_str(triple)?))
        } else {
            None
        };
        let features = cli.features.as_ref().map(|features| features.into());
        let cargo_settings = load_metadata(&current_dir)?;
        let package = Settings::find_bundle_package(cli.package.as_deref(), &cargo_settings)?;
        let bundle_settings = Settings::bundle_settings_of_package(package)?;
        let workspace_dir = Settings::get_workspace_dir(current_dir);
        let target_dir =
            Settings::get_target_dir(&workspace_dir, &target, &profile, &build_artifact);
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
            Some(PackageType::WindowsMsi) => ".exe",
            _ => "",
        };
        binary_name += binary_extension;
        let binary_path = target_dir.join(&binary_name);
        Ok(Settings {
            package: package.clone(),
            package_type,
            target,
            features,
            build_artifact,
            profile,
            all_features,
            no_default_features,
            project_out_directory: target_dir,
            binary_path,
            binary_name,
            bundle_settings,
        })
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
        target: &Option<(String, TargetInfo)>,
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

        if let &Some((ref triple, _)) = target {
            path.push(triple);
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
            Some(package) => {
                if let Some(_package) = metadata
                    .packages
                    .iter()
                    .find(|p| p.name.as_str() == package)
                {
                    return Ok(_package);
                }
                anyhow::bail!("Package '{package}' not found in workspace");
            }
            None => {
                if let Some(root_package) = metadata.root_package() {
                    return Ok(root_package);
                }
            }
        }
        anyhow::bail!("No package specified and no root package found in workspace");
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
        if let Some((_, ref info)) = self.target {
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

    /// If a specific package type was specified by the command-line, returns
    /// that package type; otherwise, if a target triple was specified by the
    /// command-line, returns the native package type(s) for that target;
    /// otherwise, returns the native package type(s) for the host platform.
    /// Fails if the host/target's native package type is not supported.
    pub fn package_types(&self) -> crate::Result<Vec<PackageType>> {
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
                "linux" => Ok(vec![PackageType::Deb, PackageType::AppImage]), // TODO: Do Rpm too, once it's implemented.
                "windows" => Ok(vec![PackageType::WindowsMsi]),
                os => anyhow::bail!("Native {} bundles not yet supported.", os),
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

    /// Returns true if the bundle is being compiled in release mode, false if
    /// it's being compiled in debug mode.
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
    pub fn icon_files(&self) -> ResourcePaths {
        match self.bundle_settings.icon {
            Some(ref paths) => ResourcePaths::new(paths.as_slice(), false),
            None => ResourcePaths::new(&[], false),
        }
    }

    /// Returns an iterator over the resource files to be included in this
    /// bundle.
    pub fn resource_files(&self) -> ResourcePaths {
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

    pub fn debian_dependencies(&self) -> &[String] {
        match self.bundle_settings.deb_depends {
            Some(ref dependencies) => dependencies.as_slice(),
            None => &[],
        }
    }

    pub fn linux_mime_types(&self) -> &[String] {
        match self.bundle_settings.linux_mime_types {
            Some(ref mime_types) => mime_types.as_slice(),
            None => &[],
        }
    }

    pub fn linux_use_terminal(&self) -> Option<bool> {
        self.bundle_settings.linux_use_terminal
    }

    pub fn linux_exec_args(&self) -> Option<&str> {
        self.bundle_settings.linux_exec_args.as_deref()
    }

    pub fn osx_frameworks(&self) -> &[String] {
        match self.bundle_settings.osx_frameworks {
            Some(ref frameworks) => frameworks.as_slice(),
            None => &[],
        }
    }

    pub fn osx_plugins(&self) -> &[String] {
        match self.bundle_settings.osx_plugins {
            Some(ref plugins) => plugins.as_slice(),
            None => &[],
        }
    }

    pub fn osx_minimum_system_version(&self) -> Option<&str> {
        self.bundle_settings.osx_minimum_system_version.as_deref()
    }

    pub fn osx_url_schemes(&self) -> &[String] {
        match self.bundle_settings.osx_url_schemes {
            Some(ref urlosx_url_schemes) => urlosx_url_schemes.as_slice(),
            None => &[],
        }
    }

    /// Returns an iterator over the plist files for this bundle
    pub fn osx_info_plist_exts(&self) -> ResourcePaths<'_> {
        match self.bundle_settings.osx_info_plist_exts {
            Some(ref paths) => ResourcePaths::new(paths.as_slice(), false),
            None => ResourcePaths::new(&[], false),
        }
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
            if let Some(ref mut walk_entries) = self.walk_iter {
                if let Some(entry) = walk_entries.next() {
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
            }
            self.walk_iter = None;
            if let Some(ref mut glob_paths) = self.glob_iter {
                if let Some(glob_result) = glob_paths.next() {
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
}
