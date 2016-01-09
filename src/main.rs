#[macro_use]
extern crate clap;
extern crate plist;
extern crate toml;

use clap::{App, AppSettings, SubCommand};
use std::env;
use std::path::{Path, PathBuf};
use std::process;

enum BuildType {
    Debug,
    Release
}

struct Settings {
    build_type: BuildType,
    project_directory: PathBuf,
    out_resource_path: PathBuf,
    resource_script: Option<PathBuf>
}

fn main() {
    let m = App::new("cargo-bundle")
                .author("George Burton <burtonageo@gmail.com>")
                .about("Bundle rust executables into OS bundles")
                .version(&*format!("v{}", crate_version!()))
                .bin_name("cargo")
                .settings(&[AppSettings::GlobalVersion, AppSettings::SubcommandRequired])
                .subcommand(SubCommand::with_name("bundle")
                    .args_from_usage(
                        "-d --resources-directory [DIR] 'Directory which contains bundle resources (images, etc)'
                         -r --release 'Build a bundle from a target built in release mode'"))
                .get_matches();

    if let Some(m) = m.subcommand_matches("bundle") {
        //let cfg = Config::from_matches(m).unwrap_or_else(|e| e.exit());
        let bundle_toml = env::current_dir().unwrap();
    }
}
