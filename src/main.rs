#[macro_use]
extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate icns;
extern crate image;
extern crate md5;
extern crate plist;
extern crate toml;
extern crate walkdir;

mod bundle;

use bundle::bundle_project;
use clap::{App, AppSettings, ArgMatches, SubCommand};
use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process;
use toml::{Parser, Table, Value};

error_chain! {
    foreign_links {
        Io(::std::io::Error);
        Image(::image::ImageError);
        Walkdir(::walkdir::Error);
    }
    errors { }
}

macro_rules! simple_parse {
    ($toml_ty:ident, $value:expr, $msg:expr) => (
        if let Value::$toml_ty(x) = $value {
            x
        } else {
            bail!(format!($msg, $value));
        }
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoSettings {
    pub project_home_directory: PathBuf,
    pub project_out_directory: PathBuf,
    pub binary_file: PathBuf,
    pub version: String,
    pub description: String,
    pub homepage: String,
    pub authors: Vec<String>
}

impl CargoSettings {
    fn new(project_home_directory: &Path, is_release: bool) -> ::Result<Self> {
        let project_dir = project_home_directory.to_path_buf();
        let mut cargo_file = None;
        for node in project_dir.read_dir()? {
            let path = node?.path();
            if let Some("Cargo.toml") = path.file_name().and_then(|fl_nm| fl_nm.to_str()) {
                cargo_file = Some(path.to_path_buf());
            }
        }

        let mut target_dir = project_dir.clone();
        let build_config = if is_release { "release" } else { "debug" };

        target_dir.push("target");
        target_dir.push(build_config);

        let cargo_info = cargo_file.ok_or("cargo.toml is not present in project directory".into())
            .and_then(load_toml)?;

        let mut settings = CargoSettings {
            project_home_directory: project_dir,
            project_out_directory: target_dir,
            binary_file: PathBuf::new(),
            version: String::new(),
            description: String::new(),
            homepage: String::new(),
            authors: Vec::new()
        };

        for (name, value) in cargo_info {
            match (&name[..], value) {
                ("package", Value::Table(table)) => {
                    for (name, value) in table {
                        match &name[..] {
                            "name" => {
                                if let Value::String(s) = value {
                                    settings.binary_file = settings.project_out_directory.clone();
                                    settings.binary_file.push(s);
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

    pub fn binary_name(&self) -> Result<String> {
        self.binary_file
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToString::to_string)
            .ok_or("Could not get file name of binary file.".into())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    pub cargo_settings: CargoSettings,
    pub package_type: Option<PackageType>, // If `None`, use the default package type for this os
    pub is_release: bool,
    pub bundle_name: String,
    pub identifier: String, // Unique identifier for the bundle
    pub version_str: Option<String>,
    pub resource_files: Vec<PathBuf>,
    pub bundle_script: Option<PathBuf>,
    pub icon_files: Vec<PathBuf>,
    pub copyright: Option<String>
}

impl Settings {
    pub fn new(current_dir: PathBuf, matches: &ArgMatches) -> ::Result<Self> {
        let is_release = matches.is_present("release");
        let cargo_settings = try!(CargoSettings::new(&current_dir, is_release));

        let mut settings = Settings {
            cargo_settings: cargo_settings,
            package_type: None,
            is_release: is_release,
            bundle_name: String::new(),
            identifier: String::new(),
            version_str: None,
            resource_files: vec![],
            bundle_script: None,
            icon_files: vec![],
            copyright: None
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
                    settings.bundle_name = simple_parse!(String,
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

        fn parse_resource_files(files_array: toml::Array) -> Result<Vec<PathBuf>> {
            fn to_file_path(file: toml::Value) -> Result<PathBuf> {
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

    pub fn version_string(&self) -> &str {
        self.version_str.as_ref().unwrap_or(&self.cargo_settings.version)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageType {
    OsxBundle,
    Deb,
    Rpm
}

fn load_toml(toml_file: PathBuf) -> Result<Table> {
    if !toml_file.exists() {
        bail!("Toml file {:?} does not exist", toml_file);
    }

    let mut toml_str = String::new();
    try!(File::open(toml_file).and_then(|mut file| file.read_to_string(&mut toml_str)));

    Ok(Parser::new(&toml_str).parse().ok_or(Error::from_kind("Could not parse Toml file".into()))?)
}


/// run `cargo build` if the binary file does not exist
fn build_project_if_unbuilt(settings: &Settings) -> Result<()> {
    let mut bin_file = settings.cargo_settings.project_out_directory.clone();
    bin_file.push(&settings.cargo_settings.binary_file);
    if !bin_file.exists() {
        // TODO(burtonageo): Should call `cargo build` here to be friendlier
        let output = process::Command::new("cargo").arg("build")
            .arg(if settings.is_release { "--release" } else { "" })
            .output()?;
        if !output.status.success() {
            bail!("Result of `cargo build` operation was unsuccessful: {}",
                  output.status);
        }
    }
    Ok(())
}

quick_main!(run);

fn run() -> ::Result<()> {
    let m = App::new("cargo-bundle")
                .author("George Burton <burtonageo@gmail.com>")
                .about("Bundle rust executables into OS bundles")
                .version(format!("v{}", crate_version!()).as_str())
                .bin_name("cargo")
                .settings(&[AppSettings::GlobalVersion, AppSettings::SubcommandRequired])
                .subcommand(SubCommand::with_name("bundle").args_from_usage(
                    "-d --resources-directory [DIR] 'Directory which contains bundle resources (images, etc)'\n\
                     -r --release 'Build a bundle from a target built in release mode'\n\
                     -f --format [FORMAT] 'Which format to use for the bundle'"))
                .get_matches();

    if let Some(m) = m.subcommand_matches("bundle") {
        let output_paths = env::current_dir().map_err(From::from)
            .and_then(|d| Settings::new(d, m))
            .and_then(|s| {
                try!(build_project_if_unbuilt(&s));
                Ok(s)
            })
            .and_then(bundle_project)?;
        let pluralised = if output_paths.len() == 1 {
            "bundle"
        } else {
            "bundles"
        };
        println!("{} {} created at:", output_paths.len(), pluralised);
        for bundle in output_paths {
            println!("\t{}", bundle.display());
        }
    }
    Ok(())
}
