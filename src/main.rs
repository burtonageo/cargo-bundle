extern crate cargo;
#[macro_use]
extern crate clap;
extern crate plist;
extern crate toml;

mod bundle;

use bundle::bundle_project;
use clap::{App, AppSettings, ArgMatches, SubCommand};
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::marker::{Send, Sync};
use std::path::{Path, PathBuf};
use std::process;
use toml::{Parser, Table, Value};

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
    pub description: String
}

impl CargoSettings {
    fn new(project_home_directory: &Path, is_release: bool) -> Result<Self, Box<Error + Send + Sync>> {
        let project_dir = project_home_directory.to_path_buf();
        let mut cargo_file = None;
        for node in try!(project_dir.read_dir()) {
            let node = try!(node);
            let path = node.path();
            if let Some("Cargo.toml") = path.file_name().and_then(|fl_nm| fl_nm.to_str()) {
                cargo_file = Some(path.to_path_buf());
            }
        }

        let mut target_dir = project_dir.clone();
        let build_config = if is_release { "release" } else { "debug" };

        target_dir.push("target");
        target_dir.push(build_config);

        if !target_dir.exists() {
            let mut err_msg = String::from(format!("Could not find correct target dir for {:?} configuration. Please \
                                                    build your project",
                                                   build_config));

            if is_release {
                err_msg.push_str(" with the --release flag.");
            } else {
                err_msg.push('.');
            }
            return Err(Box::from(err_msg));
        }

        let cargo_info = try!(cargo_file.ok_or(Box::from("Could not find Cargo.toml in project directory"))
                                        .and_then(load_toml));

        let mut settings = CargoSettings {
            project_home_directory: project_dir,
            project_out_directory: target_dir,
            binary_file: PathBuf::new(),
            version: String::new(),
            description: String::new()
        };

        for (name, value) in cargo_info {
            match &name[..] {
                "package" => {
                    if let Value::Table(table) = value {
                        for (name, value) in table {
                            match &name[..] {
                                "name" => {
                                    if let Value::String(s) = value {
                                        settings.binary_file = settings.project_out_directory.clone();
                                        settings.binary_file.push(s);
                                        if !settings.binary_file.is_file() {
                                            return Err(Box::from(format!("Built executable should be a file {:?}.",
                                                                         settings.binary_file)));
                                        }
                                    } else {
                                        return Err(Box::from(format!("Invalid format for script value in Bundle.toml: \
                                                                      Expected string, found {:?}",
                                                                     value)));
                                    }
                                }
                                "version" => {
                                    settings.version = simple_parse!(String, value,
                                                                     "Invalid format for version value in Bundle.toml: \
                                                                      Expected string, found {:?}")
                                }
                                "description" => {
                                    settings.description = simple_parse!(String, value,
                                                                         "Invalid format for description value in \
                                                                          Bundle.toml: Expected string, found {:?}")
                                }
                                _ => {}
                            }
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
    pub out_resource_path: PathBuf,
    pub bundle_script: Option<PathBuf>
}

impl Settings {
    pub fn new(current_dir: PathBuf, matches: &ArgMatches) -> Result<Self, Box<Error + Send + Sync>> {
        let is_release = matches.is_present("release");
        let cargo_settings = try!(CargoSettings::new(&current_dir, is_release));
        let out_res_path = cargo_settings.project_out_directory.clone();

        let mut settings = Settings {
            cargo_settings: cargo_settings,
            package_type: None,
            is_release: is_release,
            bundle_name: String::new(),
            identifier: String::new(),
            version_str: None,
            out_resource_path: out_res_path,
            bundle_script: None
        };

        {
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
                        settings.bundle_name = simple_parse!(String, value,
                                                             "Invalid format for bundle name value in Bundle.toml: \
                                                              Expected string, found {:?}")
                    }
                    "identifier" => {
                        settings.identifier = simple_parse!(String, value,
                                                            "Invalid format for bundle identifier value in \
                                                             Bundle.toml: Expected string, found {:?}")
                    }
                    "version" => {
                        settings.version_str = Some(simple_parse!(String, value,
                                                                  "Invalid format for bundle identifier value in \
                                                                   Bundle.toml: Expected string, found {:?}"))
                    }
                    _ => {}
                }
            }
        }

        Ok(settings)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageType {
    OsxBundle,
    Deb,
    Rpm
}

fn load_toml(toml_file: PathBuf) -> Result<Table, Box<Error + Send + Sync>> {
    if !toml_file.exists() {
        return Err(Box::from(format!("Toml file {:?} does not exist", toml_file)));
    }

    let mut toml_str = String::new();
    try!(File::open(toml_file).and_then(|mut file| file.read_to_string(&mut toml_str)));

    Ok(try!(Parser::new(&toml_str).parse().ok_or(Box::from("Could not parse Toml file"))))
}

fn main() {
    let m = App::new("cargo-bundle")
                .author("George Burton <burtonageo@gmail.com>")
                .about("Bundle rust executables into OS bundles")
                .version(&format!("v{}", crate_version!()))
                .bin_name("cargo")
                .settings(&[AppSettings::GlobalVersion, AppSettings::SubcommandRequired])
                .subcommand(SubCommand::with_name("bundle").args_from_usage(
                    "-d --resources-directory [DIR] 'Directory which contains bundle resources (images, etc)'\n\
                     -r --release 'Build a bundle from a target built in release mode'\n\
                     -f --format [FORMAT] 'Which format to use for the bundle'"))
                .get_matches();

    if let Some(m) = m.subcommand_matches("bundle") {
        env::current_dir()
            .map_err(Box::from)
            .and_then(|d| Settings::new(d, m))
            .and_then(bundle_project)
            .unwrap_or_else(|e| {
                println!("{}", e.description());
                process::exit(1);
            });
    }
}
