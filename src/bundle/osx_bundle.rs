use Settings;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File, create_dir_all};
use std::io::prelude::*;
use std::io;
use std::path::Path;
use std::marker::{Send, Sync};
use walkdir::WalkDir;

pub fn bundle_project(settings: &Settings) -> Result<(), Box<Error + Send + Sync>> {
    let mut app_bundle_path = settings.cargo_settings.project_out_directory.clone();
    app_bundle_path.push({
        let mut bundle_name = settings.bundle_name.clone();
        bundle_name.push_str(".app");
        bundle_name
    });
    app_bundle_path.push("Contents");
    try!(create_dir_all(&app_bundle_path));

    let mut plist = try!({
        let mut f = app_bundle_path.clone();
        f.push("Info.plist");
        File::create(f)
    });

    let bin_name = try!(settings.cargo_settings
                                .binary_file
                                .file_name()
                                .and_then(OsStr::to_str)
                                .map(|s| s.to_string())
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
                           "", // icon file
                           settings.bundle_name,
                           settings.version_str.as_ref().unwrap_or(&settings.cargo_settings.version),
                           settings.identifier,
                           settings.copyright.unwrap_or_default());

    try!(plist.write_all(&contents.into_bytes()[..]));
    try!(plist.sync_all());

    if !settings.resource_files.is_empty() {
        let mut resources_dir = app_bundle_path.clone();
        resources_dir.push("Resources");
        try!(create_dir_all(&resources_dir));

        for res_path in &settings.resource_files {
            try!(copy_path(&res_path, &resources_dir));
        }
    }

    let mut bin_path = app_bundle_path;
    bin_path.push("MacOS");
    try!(create_dir_all(&bin_path));
    let bundle_binary = {
        bin_path.push(bin_name);
        bin_path
    };
    try!(fs::copy(&settings.cargo_settings.binary_file, &bundle_binary));

    Ok(())
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
