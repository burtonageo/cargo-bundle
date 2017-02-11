use Settings;
use icns;
use image::{self, GenericImage};
use std::ffi::OsStr;
use std::fs::{self, File, create_dir_all};
use std::io::prelude::*;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn bundle_project(settings: &Settings) -> ::Result<Vec<PathBuf>> {
    let mut app_bundle_path = settings.cargo_settings.project_out_directory.clone();
    app_bundle_path.push({
        let mut bundle_name = settings.bundle_name.clone();
        bundle_name.push_str(".app");
        bundle_name
    });
    let mut bundle_directory = app_bundle_path.clone();
    bundle_directory.push("Contents");
    create_dir_all(&bundle_directory)?;

    let mut resources_dir = bundle_directory.clone();
    resources_dir.push("Resources");

    let bundle_icon_file: Option<PathBuf> = try!(create_icns_file(&settings.bundle_name,
                                                                  &resources_dir,
                                                                  &settings.icon_files));

    let mut plist = {
        let mut f = bundle_directory.clone();
        f.push("Info.plist");
        File::create(f)?
    };

    let bin_name = settings.cargo_settings.binary_name()?;

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
                           bundle_icon_file.as_ref()
                                           .and_then(|p| p.file_name())
                                           .and_then(OsStr::to_str)
                                           .unwrap_or("???"),
                           settings.bundle_name,
                           settings.version_string(),
                           settings.identifier,
                           settings.copyright.as_ref().unwrap_or(&String::new()));

    try!(plist.write_all(&contents.into_bytes()[..]));
    try!(plist.sync_all());

    if !settings.resource_files.is_empty() {
        try!(create_dir_all(&resources_dir));

        for res_path in &settings.resource_files {
            try!(copy_path(&res_path, &resources_dir));
        }
    }

    let mut bin_path = bundle_directory;
    bin_path.push("MacOS");
    try!(create_dir_all(&bin_path));
    let bundle_binary = {
        bin_path.push(bin_name);
        bin_path
    };
    fs::copy(&settings.cargo_settings.binary_file, &bundle_binary)?;

    Ok(vec![app_bundle_path])
}

fn copy_path(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_file() {
        // TODO(burtonageo): This fails if this is a path to a file which has directory components
        // e.g. from = `/assets/configurations/features-release.json`
        fs::copy(&from, &to)?;
        return Ok(());
    }

    for entry in WalkDir::new(from) {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        let mut destination = to.to_path_buf();
        destination.push(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&path, &destination)?;
    }

    Ok(())
}

/// Given a list of icon files, try to produce an ICNS file in the resources
/// directory and return the path to it.  Returns `Ok(None)` if no usable icons
/// were provided.
fn create_icns_file(bundle_name: &String,
                    resources_dir: &PathBuf,
                    icon_paths: &Vec<PathBuf>)
                    -> ::Result<Option<PathBuf>> {
    if icon_paths.is_empty() {
        return Ok(None);
    }

    // If one of the icon files is already an ICNS file, just use that.
    if let Some(icns_path) = icon_paths.iter().find(|path| path.extension() == Some(OsStr::new("icns"))) {
        let mut dest_path = resources_dir.to_path_buf();
        // icns_path has been verified to be a file in Settings::new
        dest_path.push(icns_path.file_name().unwrap());
        try!(create_dir_all(resources_dir));
        try!(fs::copy(&icns_path, &dest_path));
        return Ok(Some(dest_path));
    }

    // Otherwise, read available images and pack them into a new ICNS file.
    let mut family = icns::IconFamily::new();
    for icon_path in icon_paths {
        let icon = try!(image::open(icon_path));
        let density = if is_retina(icon_path) { 2 } else { 1 };
        // Try to add this image to the icon family.  Ignore images whose sizes
        // don't map to any ICNS icon type; print warnings and skip images that
        // fail to encode.
        if let Some(icon_type) = icns::IconType::from_pixel_size_and_density(icon.width(), icon.height(), density) {
            if !family.has_icon_with_type(icon_type) {
                let icon = try!(make_icns_image(icon));
                family.add_icon_with_type(&icon, icon_type).unwrap();
            }
        }
        // TODO: If none of the icons are of usable sizes, we could also use
        // the image crate to scale icons to the appropriate sizes.
    }
    if !family.is_empty() {
        try!(create_dir_all(resources_dir));
        let mut dest_path = resources_dir.clone();
        dest_path.push(bundle_name);
        dest_path.set_extension("icns");
        let icns_file = BufWriter::new(try!(File::create(&dest_path)));
        try!(family.write(icns_file));
        return Ok(Some(dest_path));
    }

    bail!("No usable icon files found.");
}

/// Converts an image::DynamicImage into an icns::Image.
fn make_icns_image(img: image::DynamicImage) -> io::Result<icns::Image> {
    let pixel_format = match img.color() {
        image::ColorType::RGBA(8) => icns::PixelFormat::RGBA,
        image::ColorType::RGB(8) => icns::PixelFormat::RGB,
        image::ColorType::GrayA(8) => icns::PixelFormat::GrayAlpha,
        image::ColorType::Gray(8) => icns::PixelFormat::Gray,
        _ => {
            let msg = format!("unsupported ColorType: {:?}", img.color());
            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
        }
    };
    icns::Image::from_data(pixel_format, img.width(), img.height(), img.raw_pixels())
}

/// Returns true if the path has a filename indicating that it is a high-desity
/// "retina" icon.  Specifically, returns true the the file stem ends with
/// "@2x" (a convention specified by the [Apple developer docs](
/// https://developer.apple.com/library/mac/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Optimizing/Optimizing.html)).
pub fn is_retina(path: &Path) -> bool {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(|stem| stem.ends_with("@2x"))
        .unwrap_or(false)
}
