use clap::ArgMatches;
use glob;
use std;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use super::common::print_warning;
use target_build_utils::TargetInfo;
use toml;
use walkdir;

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
    icon: Option<Vec<String>>,
    version: Option<String>,
    resources: Option<Vec<String>>,
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
    binary_name: String,
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
        let binary_name = match binary_path.file_name().and_then(OsStr::to_str) {
            Some(name) => name.to_string(),
            None => bail!("Could not get file name of binary file."),
        };
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
                bail!("No [package.metadata.bundle] section or Bundle.toml file found.");
            }
        };
        Ok(Settings {
            cargo_settings: cargo_settings,
            package_type: package_type,
            target: target,
            is_release: is_release,
            project_out_directory: target_dir,
            binary_path: binary_path,
            binary_name: binary_name,
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
    pub fn binary_name(&self) -> &str { &self.binary_name }

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
            allow_walk: allow_walk,
        }
    }
}

impl<'a> Iterator for ResourcePaths<'a> {
    type Item = ::Result<PathBuf>;

    fn next(&mut self) -> Option<::Result<PathBuf>> {
        loop {
            if let Some(ref mut walk_entries) = self.walk_iter {
                if let Some(entry) = walk_entries.next() {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(error) => return Some(Err(::Error::from(error))),
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
                        Err(error) => return Some(Err(::Error::from(error))),
                    };
                    if path.is_dir() {
                        if self.allow_walk {
                            let walk = walkdir::WalkDir::new(path);
                            self.walk_iter = Some(walk.into_iter());
                            continue;
                        } else {
                            let msg = format!("{:?} is a directory", path);
                            return Some(Err(::Error::from(msg)));
                        }
                    }
                    return Some(Ok(path));
                }
            }
            self.glob_iter = None;
            if let Some(pattern) = self.pattern_iter.next() {
                let glob = match glob::glob(pattern) {
                    Ok(glob) => glob,
                    Err(error) => return Some(Err(::Error::from(error))),
                };
                self.glob_iter = Some(glob);
                continue;
            }
            return None;
        }
    }
}

const BUNDLE_TOML_WARNING: &str = "\
Using Bundle.toml file, which is deprecated in favor
  of using [package.metadata.bundle] section in Cargo.toml
  file.  Support for Bundle.toml file will be removed in a
  future version of cargo-bundle.";

#[cfg(test)]
mod tests {
    use super::CargoSettings;
    use toml;

    #[test]
    fn parse_cargo_toml() {
        let toml_str = "\
            [package]\n\
            name = \"example\"\n\
            version = \"0.1.0\"\n\
            authors = [\"Jane Doe\"]\n\
            license = \"MIT\"\n\
            description = \"An example application.\"\n\
            build = \"build.rs\"\n\
            \n\
            [package.metadata.bundle]\n\
            name = \"Example Application\"\n\
            identifier = \"com.example.app\"\n\
            resources = [\"data\", \"foo/bar\"]\n\
            long_description = \"\"\"\n\
            This is an example of a\n\
            simple application.\n\
            \"\"\"\n\
            \n\
            [dependencies]\n\
            rand = \"0.4\"\n";
        let cargo_settings: CargoSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(cargo_settings.package.name, "example");
        assert_eq!(cargo_settings.package.version, "0.1.0");
        assert_eq!(cargo_settings.package.description,
                   "An example application.");
        assert_eq!(cargo_settings.package.homepage, None);
        assert_eq!(cargo_settings.package.authors,
                   Some(vec!["Jane Doe".to_string()]));
        assert!(cargo_settings.package.metadata.is_some());
        let metadata = cargo_settings.package.metadata.as_ref().unwrap();
        assert!(metadata.bundle.is_some());
        let bundle = metadata.bundle.as_ref().unwrap();
        assert_eq!(bundle.name, Some("Example Application".to_string()));
        assert_eq!(bundle.identifier, Some("com.example.app".to_string()));
        assert_eq!(bundle.icon, None);
        assert_eq!(bundle.version, None);
        assert_eq!(bundle.resources,
                   Some(vec!["data".to_string(), "foo/bar".to_string()]));
        assert_eq!(bundle.long_description,
                   Some("This is an example of a\n\
                         simple application.\n".to_string()));
    }
}
