#[macro_use]
extern crate clap;
#[macro_use]
extern crate hyper;
extern crate plist;
extern crate toml;
extern crate url;
extern crate walkdir;

mod bundle;

use bundle::bundle_project;
use clap::{App, AppSettings, ArgMatches, SubCommand};
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::stderr;
use std::io::prelude::*;
use std::marker::{Send, Sync};
use std::path::{Path, PathBuf};
use std::process;
use toml::{Parser, Table, Value};
use url::Url;

macro_rules! simple_parse {
    ($toml_ty:ident, $value:expr, $msg:expr) => (
        if let Value::$toml_ty(x) = $value {
            x
        } else {
            return Err(Box::from(format!($msg, $value)));
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
    fn new(project_home_directory: &Path, is_release: bool) -> Result<Self, Box<Error + Send + Sync>> {
        let project_dir = project_home_directory.to_path_buf();
        let mut cargo_file = None;
        for node in try!(project_dir.read_dir()) {
            let path = try!(node).path();
            if let Some("Cargo.toml") = path.file_name().and_then(|fl_nm| fl_nm.to_str()) {
                cargo_file = Some(path.to_path_buf());
            }
        }

        let mut target_dir = project_dir.clone();
        let build_config = if is_release { "release" } else { "debug" };

        target_dir.push("target");
        target_dir.push(build_config);

        let cargo_info = try!(cargo_file.ok_or(Box::from("Could not find Cargo.toml in project directory"))
                                        .and_then(load_toml));

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
                                    return Err(Box::from(format!("Invalid format for script value in Bundle.toml: \
                                                                  Expected string, found {:?}",
                                                                 value)));
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
                                                        .filter_map(|v| {
                                                            if let Value::String(s) = v { Some(s) } else { None }
                                                        })
                                                        .collect();
                                } else {
                                    return Err(Box::from(format!("Invalid format for script value in Bundle.toml: \
                                                                  Expected array, found {:?}",
                                                                 value)));
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    pub cargo_settings: CargoSettings,
    pub package_type: Option<PackageType>, // If `None`, use the default package type for this os
    pub is_release: bool,
    pub bundle_name: String,
    pub identifier: String, // Unique identifier for the bundle
    pub version_str: Option<String>,
    pub resources: Vec<Resource>,
    pub bundle_script: Option<PathBuf>,
    pub icon_file: Option<PathBuf>,
    pub copyright: Option<String>
}

impl Settings {
    pub fn new(current_dir: PathBuf, matches: &ArgMatches) -> Result<Self, Box<Error + Send + Sync>> {
        let is_release = matches.is_present("release");
        let cargo_settings = try!(CargoSettings::new(&current_dir, is_release));

        let mut settings = Settings {
            cargo_settings: cargo_settings,
            package_type: None,
            is_release: is_release,
            bundle_name: String::new(),
            identifier: String::new(),
            version_str: None,
            resources: vec![],
            bundle_script: None,
            icon_file: None,
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
                            return Err(Box::from(format!("{:?} should be a file", path)));
                        }
                    } else {
                        return Err(Box::from(format!("Invalid format for script value in Bundle.toml: \
                                                      Expected string, found {:?}",
                                                     value)));
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
                    let icon_path = simple_parse!(String,
                                                  value,
                                                  "Invalid format for bundle identifier value in \
                                                   Bundle.toml: Expected string, found {:?}");
                    let icon_path = PathBuf::from(icon_path);
                    if !icon_path.is_file() {
                        return Err(Box::from("The Icon attribute must point to a file"));
                    }
                    settings.icon_file = Some(icon_path);
                }
                "resources" => {
                    let files = simple_parse!(Array,
                                              value,
                                              "Invalid format for bundle resource files format in \
                                               Bundle.toml: Expected array, found {:?}");
                    settings.resources = try!(parse_resources(files))
                }
                _ => {}
            }
        }

        fn parse_resources(files_array: toml::Array) -> Result<Vec<Resource>, Box<Error + Send + Sync>> {
            fn to_file_path(file: toml::Value) -> Result<Resource, Box<Error + Send + Sync>> {
                if let Value::String(s) = file {
                    let path = PathBuf::from(s);
                    if !path.exists() {
                        return Err(Box::from(format!("Resource file {} does not exist.", path.display())));
                    } else {
                        Ok(Resource::LocalFile(path))
                    }
                } else {
                    return Err(Box::from("Invalid format for resource."));
                }
            };

            let mut out_files = Vec::with_capacity(files_array.len());
            for file in files_array.into_iter().map(to_file_path) {
                match file {
                    Ok(file) => out_files.push(file),
                    Err(e) => return Err(e),
                }
            }
            Ok(out_files)
        }

        Ok(settings)
    }

    /// Get the final list of resource files from the file system. If not all of the
    /// resource files have been downloaded, it returns `None`.
    pub fn get_resolved_files(&self) -> Option<Vec<PathBuf>> {
        unimplemented!();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageType {
    OsxBundle,
    Deb,
    Rpm
}

/// A representation of a Resource file from the `Bundle.toml` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Resource {
    /// A remote resource which will be retreived from a URL.
    Remote(Url),

    /// A file on the file system.
    LocalFile(PathBuf)
}

impl Resource {
    fn get_local_file(&self) -> Option<&Path> {
        match *self {
            Resource::Remote(_) => unimplemented!(),
            Resource::LocalFile(ref path) => Some(path),
        }
    }
}

fn load_toml(toml_file: PathBuf) -> Result<Table, Box<Error + Send + Sync>> {
    if !toml_file.exists() {
        return Err(Box::from(format!("Toml file {:?} does not exist", toml_file)));
    }

    let mut toml_str = String::new();
    try!(File::open(toml_file).and_then(|mut file| file.read_to_string(&mut toml_str)));

    Ok(try!(Parser::new(&toml_str)
                .parse()
                .ok_or(Box::<Error + Send + Sync>::from("Could not parse Toml file"))))
}


/// run `cargo build` if the binary file does not exist
fn build_project_if_unbuilt(settings: &Settings) -> Result<(), Box<Error + Send + Sync>> {
    let mut bin_file = settings.cargo_settings.project_out_directory.clone();
    bin_file.push(&settings.cargo_settings.binary_file);
    if !bin_file.exists() {
        // TODO(burtonageo): Should call `cargo build` here to be friendlier
        let output = try!(process::Command::new("cargo")
                              .arg("build")
                              .arg(if settings.is_release { "--release" } else { "" })
                              .output()
                              .map_err(Box::<Error + Send + Sync>::from));
        if !output.status.success() {
            return Err(Box::from("Result of `cargo build` operation was unsuccessful"));
        }
    }
    Ok(())
}

fn main() {
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
        let output_paths = env::current_dir()
                               .map_err(Box::from)
                               .and_then(|d| Settings::new(d, m))
                               .and_then(|s| {
                                   try!(build_project_if_unbuilt(&s));
                                   Ok(s)
                               })
                               .and_then(bundle_project)
                               .unwrap_or_else(|e| {
                                   let _ = write!(stderr(), "{}", e.description());
                                   process::exit(1);
                               });
        let pluralised = if output_paths.len() == 1 { "bundle" } else { "bundles" };
        println!("{} {} created at:", output_paths.len(), pluralised);
        for bundle in output_paths {
            println!("\t{}", bundle.display());
        }
    }
}
