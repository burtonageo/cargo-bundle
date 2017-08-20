use clap::ArgMatches;
use std;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use target_build_utils::TargetInfo;
use toml::{self, Value};

macro_rules! simple_parse {
    ($toml_ty:ident, $value:expr, $msg:expr) => (
        if let Value::$toml_ty(x) = $value {
            x
        } else {
            bail!(format!($msg, $value));
        }
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageType {
    OsxBundle,
    IosBundle,
    Deb,
    Rpm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CargoSettings {
    project_home_directory: PathBuf,
    project_out_directory: PathBuf,
    name: String,
    binary_file: PathBuf,
    version: String,
    description: String,
    homepage: String,
    authors: Vec<String>,
}

impl CargoSettings {
    fn new(project_home_directory: &Path, target_triple: Option<&str>, is_release: bool) -> ::Result<Self> {
        let project_dir = project_home_directory.to_path_buf();
        let mut cargo_file = None;
        for node in project_dir.read_dir()? {
            let path = node?.path();
            if let Some("Cargo.toml") = path.file_name().and_then(|fl_nm| fl_nm.to_str()) {
                cargo_file = Some(path.to_path_buf());
            }
        }

        let mut target_dir = project_home_directory.join("target");
        if let Some(triple) = target_triple {
            target_dir.push(triple);
        }
        target_dir.push(if is_release { "release" } else { "debug" });

        let cargo_info = cargo_file.ok_or("cargo.toml is not present in project directory".into()).and_then(load_toml)?;

        let mut settings = CargoSettings {
            project_home_directory: project_dir,
            project_out_directory: target_dir,
            name: String::new(),
            binary_file: PathBuf::new(),
            version: String::new(),
            description: String::new(),
            homepage: String::new(),
            authors: Vec::new(),
        };

        for (name, value) in cargo_info {
            match (&name[..], value) {
                ("package", Value::Table(table)) => {
                    for (name, value) in table {
                        match &name[..] {
                            "name" => {
                                if let Value::String(s) = value {
                                    settings.binary_file = settings.project_out_directory.clone();
                                    settings.binary_file.push(&s);
                                    settings.name = s;
                                } else {
                                    bail!("expected field \"name\" to have type \"String\", actually has \
                                           type {}",
                                          value);
                                }
                            }
                            "version" => {
                                settings.version = simple_parse!(String,
                                                                 value,
                                                                 "Invalid format for version value in Bundle.toml: \
                                                                  Expected string, found {:?}")
                            }
                            "description" => {
                                settings.description = simple_parse!(String,
                                                                     value,
                                                                     "Invalid format for description value in \
                                                                      Bundle.toml: Expected string, found {:?}")
                            }
                            "homepage" => {
                                settings.homepage = simple_parse!(String,
                                                                  value,
                                                                  "Invalid format for description value in \
                                                                   Bundle.toml: Expected string, found {:?}")
                            }
                            "authors" => {
                                if let Value::Array(a) = value {
                                    settings.authors = a.into_iter()
                                        .filter_map(|v| if let Value::String(s) = v {
                                                        Some(s)
                                                    } else {
                                                        None
                                                    })
                                        .collect();
                                } else {
                                    bail!("Invalid format for script value in Bundle.toml: \
                                           Expected array, found {:?}",
                                          value);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(settings)
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    cargo_settings: CargoSettings,
    package_type: Option<PackageType>, // If `None`, use the default package type for this os
    target: Option<(String, TargetInfo)>,
    is_release: bool,
    name: String,
    identifier: String, // Unique identifier for the bundle
    version_str: Option<String>,
    resource_files: Vec<PathBuf>,
    bundle_script: Option<PathBuf>,
    icon_files: Vec<PathBuf>,
    copyright: Option<String>,
    short_desc: Option<String>,
    long_desc: Option<String>,
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
        let cargo_settings = CargoSettings::new(&current_dir,
                                                target.as_ref().map(|&(ref triple, _)| triple.as_str()),
                                                is_release)?;

        let mut settings = Settings {
            cargo_settings: cargo_settings,
            package_type: package_type,
            target: target,
            is_release: is_release,
            name: String::new(),
            identifier: String::new(),
            version_str: None,
            resource_files: vec![],
            bundle_script: None,
            icon_files: vec![],
            copyright: None,
            short_desc: None,
            long_desc: None,
        };

        let table = try!({
                             let mut f = current_dir.clone();
                             f.push("Bundle.toml");
                             load_toml(f)
                         });

        for (name, value) in table {
            match &name[..] {
                "script" => {
                    if let Value::String(s) = value {
                        let path = PathBuf::from(s);
                        if path.is_file() {
                            settings.bundle_script = Some(path);
                        } else {
                            bail!("{:?} must be a file", path);
                        }
                    } else {
                        bail!("Invalid format for script value in Bundle.toml: \
                               Expected string, found {:?}",
                              value);
                    }
                }
                "name" => {
                    settings.name = simple_parse!(String,
                                                  value,
                                                  "Invalid format for bundle name value in Bundle.toml: \
                                                   Expected string, found {:?}")
                }
                "identifier" => {
                    settings.identifier = simple_parse!(String,
                                                        value,
                                                        "Invalid format for bundle identifier value in \
                                                         Bundle.toml: Expected string, found {:?}")
                }
                "version" => {
                    settings.version_str = Some(simple_parse!(String,
                                                              value,
                                                              "Invalid format for version value in \
                                                               Bundle.toml: Expected string, found {:?}"))
                }
                "copyright" => {
                    settings.copyright = Some(simple_parse!(String,
                                                            value,
                                                            "Invalid format for copyright notice in \
                                                             Bundle.toml: Expected string, found {:?}"))
                }
                "short_description" => {
                    settings.short_desc = Some(simple_parse!(String,
                                                             value,
                                                             "Invalid format for short description in \
                                                              Bundle.toml: Expected string, found {:?}"))
                }
                "long_description" => {
                    settings.long_desc = Some(simple_parse!(String,
                                                            value,
                                                            "Invalid format for long description in \
                                                             Bundle.toml: Expected string, found {:?}"))
                }
                "icon" => {
                    settings.icon_files = match value {
                        Value::String(icon_path) => {
                            let icon_path = PathBuf::from(icon_path);
                            if !icon_path.is_file() {
                                bail!("The icon attribute must point to a file");
                            }
                            vec![icon_path]
                        }
                        Value::Array(icon_paths) => try!(parse_resource_files(icon_paths)),
                        _ => {
                            bail!("Invalid format for bundle icon in Bundle.toml: Expected string or \
                                   array, found {:?}",
                                  value);
                        }
                    };
                }
                "resources" => {
                    let files = simple_parse!(Array,
                                              value,
                                              "Invalid format for bundle resource files format in \
                                               Bundle.toml: Expected array, found {:?}");
                    settings.resource_files = parse_resource_files(files)?
                }
                _ => {}
            }
        }

        fn parse_resource_files(files_array: toml::value::Array) -> ::Result<Vec<PathBuf>> {
            fn to_file_path(file: toml::Value) -> ::Result<PathBuf> {
                if let Value::String(s) = file {
                    let path = PathBuf::from(s);
                    if !path.exists() {
                        bail!("Resource file {} does not exist.", path.display());
                    } else {
                        Ok(path)
                    }
                } else {
                    bail!("Invalid format for resource.");
                }
            }

            let mut out_files = Vec::with_capacity(files_array.len());
            for file in files_array.into_iter().map(to_file_path) {
                out_files.push(file?);
            }
            Ok(out_files)
        }
        Ok(settings)
    }

    /// Returns the directory where the bundle should be placed.
    pub fn project_out_directory(&self) -> &Path {
        &self.cargo_settings.project_out_directory
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
        self.binary_path()
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToString::to_string)
            .ok_or("Could not get file name of binary file.".into())
    }

    /// Returns the path to the binary being bundled.
    pub fn binary_path(&self) -> &Path { &self.cargo_settings.binary_file }

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
        if self.name.is_empty() {
            &self.cargo_settings.name
        } else {
            &self.name
        }
    }

    pub fn bundle_identifier(&self) -> &str { &self.identifier }

    /// Returns an iterator over the icon files to be used for this bundle.
    pub fn icon_files(&self) -> std::slice::Iter<PathBuf> {
        self.icon_files.iter()
    }

    /// Returns an iterator over the resource files to be included in this
    /// bundle.
    pub fn resource_files(&self) -> std::slice::Iter<PathBuf> {
        self.resource_files.iter()
    }

    pub fn version_string(&self) -> &str {
        self.version_str.as_ref().unwrap_or(&self.cargo_settings.version)
    }

    pub fn copyright_string(&self) -> Option<&str> {
        self.copyright.as_ref().map(String::as_str)
    }

    pub fn author_names(&self) -> std::slice::Iter<String> {
        self.cargo_settings.authors.iter()
    }

    pub fn homepage_url(&self) -> &str { &self.cargo_settings.homepage }

    pub fn short_description(&self) -> &str {
        self.short_desc.as_ref().unwrap_or(&self.cargo_settings.description)
    }

    pub fn long_description(&self) -> Option<&str> {
        self.long_desc.as_ref().map(String::as_str)
    }
}

fn load_toml(toml_file: PathBuf) -> ::Result<toml::value::Table> {
    if !toml_file.exists() {
        bail!("Toml file {:?} does not exist", toml_file);
    }

    let mut toml_str = String::new();
    try!(File::open(toml_file).and_then(|mut file| file.read_to_string(&mut toml_str)));

    match toml_str.parse::<Value>()? {
        toml::Value::Table(table) => Ok(table),
        _ => Err(::Error::from_kind("Could not parse Toml file".into()))
    }
}
