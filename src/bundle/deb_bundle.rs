use {CargoSettings, Settings};
use std::env;
use std::error::Error;
use std::fs;
use std::marker::{Send, Sync};
use std::path::PathBuf;

pub fn bundle_project(settings: &Settings) -> Result<Vec<PathBuf>, Box<Error + Send + Sync>> {
    fn get_homepage(settings: &CargoSettings) -> &str {
        if !settings.description.is_empty() {
            &settings.description
        } else if !settings.homepage.is_empty() {
            &settings.homepage
        } else {
            &""
        }
    }

    let deb_path = settings.cargo_settings.project_out_directory.clone();
    let bin_file = try!(fs::File::open(&settings.cargo_settings.binary_file));
    let bin_file_metadata = try!(bin_file.metadata());

    let control_file_contents = format!(
        "Package: {}\n
         Version: {}\n
         Architecture: {}\n
         Maintainer: {}\n
         Installed-Size: {}\n
         Depends: {}\n
         Suggests: {}\n
         Conflicts: {}\n
         Breaks: {}\n
         Replaces: {}\n
         Provides: {}\n
         Section: {}\n
         Priority: {}\n
         Homepage: {}\n
         Description: {}",
        settings.bundle_name,
        settings.cargo_settings.version,
        env::consts::ARCH, // TODO(burtonageo): Use binary arch rather than host arch
        settings.cargo_settings.authors.iter().fold(String::new(), |mut acc, s| {
            acc.push_str(&s);
            acc
        }),
        bin_file_metadata.len(), // TODO(burtonageo): Compute data size
        "deps",
        "suggests",
        "conflicts",
        "breaks",
        "replaces",
        "provides",
        "section",
        "priority",
        get_homepage(&settings.cargo_settings),
        settings.cargo_settings.description);

    unimplemented!();
}
