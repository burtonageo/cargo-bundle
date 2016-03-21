use Settings;
use std::error::Error;
use std::marker::{Send, Sync};
use std::path::PathBuf;

const CONTROL_FILE_FMT: &'static str = "
Package: {}
Version: {}
Architecture: {}
Maintainer: {}
Installed-Size: {}
Depends: {}
Suggests: {}
Conflicts: {}
Breaks: {}
Replaces: {}
Provides: {}
Section: {}
Priority: {}
Homepage: {}
Description: {}
";

pub fn bundle_project(settings: &Settings) -> Result<Vec<PathBuf>, Box<Error + Send + Sync>> {
    let deb_path = settings.cargo_settings.project_out_directory.clone();

    unimplemented!();
}
