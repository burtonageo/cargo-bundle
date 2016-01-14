use Settings;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File, create_dir_all};
use std::io::prelude::*;
use std::marker::{Send, Sync};

pub fn bundle_project(settings: &Settings) -> Result<(), Box<Error + Send + Sync>> {
    let mut app_bundle_path = settings.cargo_settings.project_out_directory.clone();
    app_bundle_path.push({
        let mut bundle_name = settings.bundle_name.clone();
        bundle_name.push_str(".app");
        bundle_name
    });
    app_bundle_path.push("Contents");
    try!(create_dir_all(&app_bundle_path).map_err(Box::from));

    let mut plist = try!({
        let mut f = app_bundle_path.clone();
        f.push("Info.plist");
        File::create(f).map_err(Box::from)
    });

    let bin_name = try!(settings.cargo_settings
                                .binary_file
                                .file_name()
                                .and_then(OsStr::to_str)
                                .map(|s| s.to_string())
                                .ok_or(Box::from("Could not get file name of binary file.")));

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
                           "" /* copyright */);

    try!(plist.write_all(&contents.into_bytes()[..]).map_err(Box::from));
    try!(plist.sync_all().map_err(Box::from));

    app_bundle_path.push("MacOS");
    try!(create_dir_all(&app_bundle_path).map_err(Box::from));
    let bundle_binary = {
        app_bundle_path.push(bin_name);
        app_bundle_path
    };

    try!(fs::copy(&settings.cargo_settings.binary_file, &bundle_binary).map_err(Box::from));

    Ok(())
}
