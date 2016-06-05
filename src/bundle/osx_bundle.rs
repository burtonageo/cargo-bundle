use Settings;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File, create_dir_all};
use std::io::prelude::*;
use std::io;
use std::path::{Path, PathBuf};
use std::marker::{Send, Sync};
use walkdir::WalkDir;

pub fn bundle_project(settings: &Settings) -> Result<Vec<PathBuf>, Box<Error + Send + Sync>> {
    let mut app_bundle_path = settings.cargo_settings.project_out_directory.clone();
    app_bundle_path.push({
        let mut bundle_name = settings.bundle_name.clone();
        bundle_name.push_str(".app");
        bundle_name
    });
    let mut bundle_directory = app_bundle_path.clone();
    bundle_directory.push("Contents");
    try!(create_dir_all(&bundle_directory));

    let mut plist = try!({
        let mut f = bundle_directory.clone();
        f.push("Info.plist");
        File::create(f)
    });

    let bin_name = try!(settings.cargo_settings
                                .binary_file
                                .file_name()
                                .and_then(OsStr::to_str)
                                .map(ToString::to_string)
                                .ok_or(Box::from("Could not get file name of binary file.")
                                            as Box<Error + Send + Sync>));

    let contents = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                            <!DOCTYPE plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \
                                        \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                            <plist version=\"1.0\">\n\
                            <dict>\n\
                                <key>CFBundleDevelopmentRegion</key>\n\
                                <string>English</string>\n\
                                <key>CFBundleExecutable</key>\n\
                                <string>{}</string>\n\
                                <key>CFBundleGetInfoString</key>\n\
                                <string></string>\n\
                                <key>CFBundleIconFile</key>\n\
                                <string>{}</string>\n\
                                <key>CFBundleIdentifier</key>\n\
                                <string></string>\n\
                                <key>CFBundleInfoDictionaryVersion</key>\n\
                                <string>6.0</string>\n\
                                <key>CFBundleLongVersionString</key>\n\
                                <string></string>\n\
                                <key>CFBundleName</key>\n\
                                <string>{}</string>\n\
                                <key>CFBundlePackageType</key>\n\
                                <string>APPL</string>\n\
                                <key>CFBundleShortVersionString</key>\n\
                                <string>{}</string>\n\
                                <key>CFBundleSignature</key>\n\
                                <string>{}</string>\n\
                                <key>CFBundleVersion</key>\n\
                                <string></string>\n\
                                <key>CSResourcesFileMapped</key>\n\
                                <true/>\n\
                                <key>LSRequiresCarbon</key>\n\
                                <true/>\n\
                                <key>NSHumanReadableCopyright</key>\n\
                                <string>{}</string>\n\
                            </dict>\n\
                            </plist>",
                           bin_name,
                           settings.icon_file.as_ref()
                                             .and_then(|p| p.file_name())
                                             .and_then(OsStr::to_str)
                                             .unwrap_or("???"),
                           settings.bundle_name,
                           settings.version_str.as_ref().unwrap_or(&settings.cargo_settings.version),
                           settings.identifier,
                           settings.copyright.as_ref().unwrap_or(&String::new()));

    try!(plist.write_all(&contents.into_bytes()[..]));
    try!(plist.sync_all());

    let mut resources_dir = bundle_directory.clone();
    resources_dir.push("Resources");

    if !settings.resource_files.is_empty() || settings.icon_file.is_some() {
        try!(create_dir_all(&resources_dir));
    }

    if resources_dir.exists() {
        for res_path in &settings.resource_files {
            try!(copy_path(&res_path, &resources_dir));
        }

        if let Some(ref icon_file) = settings.icon_file {
            let mut bundle_icon_file = resources_dir.clone();
            // icon_file has been verified to be a file in Settings::new
            bundle_icon_file.push(icon_file.file_name().unwrap());
            try!(File::create(bundle_icon_file.clone()));
            try!(fs::copy(&icon_file, &bundle_icon_file));
        }
    }

    let mut bin_path = bundle_directory;
    bin_path.push("MacOS");
    try!(create_dir_all(&bin_path));
    let bundle_binary = {
        bin_path.push(bin_name);
        bin_path
    };
    try!(fs::copy(&settings.cargo_settings.binary_file, &bundle_binary));

    Ok(vec![app_bundle_path])
}

fn copy_path(from: &Path, to: &Path) -> Result<(), io::Error> {
    if from.is_file() {
        // TODO(burtonageo): This fails if this is a path to a file which has directory components
        // e.g. from = `/assets/configurations/features-release.json`
        try!(fs::copy(&from, &to));
        return Ok(());
    }

    for entry in WalkDir::new(from) {
        let entry = try!(entry);
        let entry = entry.path();

        if entry.is_dir() {
            continue;
        }

        let mut entry_destination = to.to_path_buf();
        entry_destination.push(entry);
        if let Some(parent) = entry_destination.parent() {
            try!(fs::create_dir_all(parent));
        }
        try!(fs::copy(&entry, &entry_destination));
    }

    Ok(())
}
