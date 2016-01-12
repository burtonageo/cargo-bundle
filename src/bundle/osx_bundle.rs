use Settings;
use std::error::Error;
use std::fs::{File, copy, create_dir_all};
use std::io::prelude::*;
use std::marker::{Send, Sync};

const PLIST_TEMPLATE: &'static str = &"
    <?xml version=\"1.0\" encoding=\"UTF-8\"?>\
    <!DOCTYPE plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\
    <plist version=\"1.0\">\
    <dict>\
        <key>CFBundleDevelopmentRegion</key>\
        <string>English</string>\
        <key>CFBundleExecutable</key>\
        <string>{:?}</string>\
        <key>CFBundleGetInfoString</key>\
        <string></string>\
        <key>CFBundleIconFile</key>\
        <string>{:?}</string>\
        <key>CFBundleIdentifier</key>\
        <string></string>\
        <key>CFBundleInfoDictionaryVersion</key>\
        <string>6.0</string>\
        <key>CFBundleLongVersionString</key>\
        <string></string>\
        <key>CFBundleName</key>\
        <string>{:?}</string>\
        <key>CFBundlePackageType</key>\
        <string>APPL</string>\
        <key>CFBundleShortVersionString</key>\
        <string>{:?}</string>\
        <key>CFBundleSignature</key>\
        <string>{:?}</string>\
        <key>CFBundleVersion</key>\
        <string></string>\
        <key>CSResourcesFileMapped</key>\
        <true/>\
        <key>LSRequiresCarbon</key>\
        <true/>\
        <key>NSHumanReadableCopyright</key>\
        <string>{:?}</string>\
    </dict>\
    </plist>";

pub fn bundle_project(settings: &Settings) -> Result<(), Box<Error + Send + Sync>> {
    let mut app_bundle_path = settings.cargo_settings.project_out_directory.clone();
    app_bundle_path.push({
        let mut bundle_name = settings.bundle_name.clone();
        bundle_name.push_str(".app");
        bundle_name
    });
    app_bundle_path.push("Contents");
    create_dir_all(&app_bundle_path);

    let mut plist = try!({
        let mut f = app_bundle_path.clone();
        f.push("Info.plist");
        File::create(f).map_err(Box::from)
    });

    {
        // write plist...
    }

    app_bundle_path.push("MacOS");
    create_dir_all(&app_bundle_path);
    println!("{:?}", settings.cargo_settings.binary_file);
    let _ = try!(copy(&settings.cargo_settings.binary_file, app_bundle_path).map_err(Box::from));

    Ok(())
}
