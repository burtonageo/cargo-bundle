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
use toml::{Parser, Value};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoSettings {
    pub project_home_directory: PathBuf,
    pub project_out_directory: PathBuf
}

impl CargoSettings {
    fn new(project_home_directory: &Path) -> Result<Self, Box<Error + Send + Sync>> {
        let project_dir = project_home_directory.to_path_buf();
        let mut cargo_file = None;
        for node in try!(project_dir.read_dir()) {
            let node = try!(node);
            let path = node.path();
            if let Some("Cargo.toml") = path.file_name().and_then(|fl_nm| fl_nm.to_str()) {
                cargo_file = Some(path.to_path_buf());
            }
        }

        let _cargo_file = try!(cargo_file.ok_or(Box::from("Could not find Cargo.toml in project directory")));
        let mut target_dir = project_dir.clone();

        target_dir.push("target");
        Ok(CargoSettings {
            project_home_directory: project_dir,
            project_out_directory: target_dir
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    pub cargo_settings: CargoSettings,
    pub package_type: Option<PackageType>, // If `None`, use the default package type for this os
    pub is_release: bool,
    pub bundle_name: String,
    pub out_resource_path: PathBuf,
    pub bundle_script: Option<PathBuf>
}

impl Settings {
    pub fn new(current_dir: PathBuf, matches: &ArgMatches) -> Result<Self, Box<Error + Send + Sync>> {
        let is_release = matches.is_present("release");
        let cargo_settings = try!(CargoSettings::new(&current_dir));
        let out_res_path = cargo_settings.project_out_directory.clone();

        let mut settings = Settings {
            cargo_settings: cargo_settings,
            package_type: None,
            is_release: is_release,
            bundle_name: String::new(),
            out_resource_path: out_res_path,
            bundle_script: None
        };
        
        {
            let mut toml_str = String::new();
            let mut toml = {
                let mut toml_bundle_path = current_dir.clone();
                toml_bundle_path.push("Bundle.toml");
            
                if !toml_bundle_path.exists() {
                    return Err(Box::from(format!("Could not find Bundle.toml file in path {:?}", current_dir)));
                }
            
                try!(File::open(toml_bundle_path).and_then(|mut file| file.read_to_string(&mut toml_str)));
                Parser::new(&toml_str)
            };
            
            let table = try!(toml.parse().ok_or(Box::from("Could not parse Toml file")));
    
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
                            return Err(Box::from(format!("Invalid format for script value in Bundle.toml:
                                                          Expected string, found {:?}",
                                                         value)));
                        }
                    }
                    "name" => {
                        if let Value::String(s) = value {
                            settings.bundle_name = s;
                        } else {
                            return Err(Box::from(format!("Invalid format for bundle name value in Bundle.toml:
                                                          Expected string, found {:?}",
                                                         value)));
                        }
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

fn main() {
    let m = App::new("cargo-bundle")
                .author("George Burton <burtonageo@gmail.com>")
                .about("Bundle rust executables into OS bundles")
                .version(&format!("v{}", crate_version!()))
                .bin_name("cargo")
                .settings(&[AppSettings::GlobalVersion, AppSettings::SubcommandRequired])
                .subcommand(SubCommand::with_name("bundle").args_from_usage(
                    "-d --resources-directory [DIR] 'Directory which contains bundle resources (images, etc)'
                     -r --release 'Build a bundle from a target built in release mode'
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
