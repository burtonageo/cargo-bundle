// An iOS package is laid out like:
//
// Foobar.app         # Actually a directory
//     Foobar             # The main binary executable of the app
//     Info.plist         # An XML file containing the app's metadata
//     ...                # Icons and other resource files
//
// See https://developer.apple.com/go/?id=bundle-structure for a full
// explanation.

use super::common;
use Settings;
use icns;
use image::{self, GenericImage, ImageDecoder};
use image::png::{PNGDecoder, PNGEncoder};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn bundle_project(settings: &Settings) -> ::Result<Vec<PathBuf>> {
    let app_bundle_name = format!("{}.app", settings.bundle_name());
    common::print_bundling(&app_bundle_name)?;
    let bundle_dir = settings.cargo_settings.project_out_directory.join(&app_bundle_name);
    fs::create_dir_all(&bundle_dir)?;
    for res_path in &settings.resource_files {
        common::copy_to_dir(res_path, &bundle_dir.join("Resources"))?;
    }
    let icon_filenames = generate_icon_files(&bundle_dir, settings)?;
    generate_info_plist(&bundle_dir, settings, &icon_filenames)?;
    let bin_path = bundle_dir.join(&settings.bundle_name());
    fs::copy(&settings.cargo_settings.binary_file, bin_path)?;
    Ok(vec![bundle_dir])
}

/// Generate the icon files and store them under the `bundle_dir`.
fn generate_icon_files(bundle_dir: &Path, settings: &Settings) -> ::Result<Vec<String>> {
    let mut filenames = Vec::new();
    {
        let mut get_dest_path = |width: u32, height: u32, is_retina: bool| {
            let filename = format!("icon_{}x{}{}.png",
                                   width,
                                   height,
                                   if is_retina { "@2x" } else { "" });
            let path = bundle_dir.join(&filename);
            filenames.push(filename);
            path
        };
        let mut sizes = BTreeSet::new();
        // Prefer PNG files.
        for icon_path in settings.icon_files.iter().filter(|path| path.extension() == Some(OsStr::new("png"))) {
            let mut decoder = PNGDecoder::new(File::open(icon_path)?);
            let (width, height) = decoder.dimensions()?;
            let is_retina = common::is_retina(icon_path);
            if !sizes.contains(&(width, height, is_retina)) {
                sizes.insert((width, height, is_retina));
                let dest_path = get_dest_path(width, height, is_retina);
                fs::copy(icon_path, dest_path)?;
            }
        }
        // Fall back to non-PNG files for any missing sizes.
        for icon_path in settings.icon_files.iter().filter(|path| path.extension() != Some(OsStr::new("png"))) {
            if icon_path.extension() == Some(OsStr::new("icns")) {
                let icon_family = icns::IconFamily::read(File::open(icon_path)?)?;
                for icon_type in icon_family.available_icons() {
                    let width = icon_type.screen_width();
                    let height = icon_type.screen_height();
                    let is_retina = icon_type.pixel_density() > 1;
                    if !sizes.contains(&(width, height, is_retina)) {
                        sizes.insert((width, height, is_retina));
                        let dest_path = get_dest_path(width, height, is_retina);
                        let icon = icon_family.get_icon_with_type(icon_type)?;
                        icon.write_png(File::create(dest_path)?)?;
                    }
                }
            } else {
                let icon = try!(image::open(icon_path));
                let (width, height) = icon.dimensions();
                let is_retina = common::is_retina(icon_path);
                if !sizes.contains(&(width, height, is_retina)) {
                    sizes.insert((width, height, is_retina));
                    let dest_path = get_dest_path(width, height, is_retina);
                    let encoder = PNGEncoder::new(common::create_file(&dest_path)?);
                    encoder.encode(&icon.raw_pixels(), width, height, icon.color())?;
                }
            }
        }
    }
    Ok(filenames)
}

fn generate_info_plist(bundle_dir: &Path, settings: &Settings, icon_filenames: &Vec<String>) -> ::Result<()> {
    let file = &mut common::create_file(&bundle_dir.join("Info.plist"))?;
    write!(file,
           "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
            <!DOCTYPE plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \
            \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
            <plist version=\"1.0\">\n\
            <dict>\n")?;
    write!(file,
           "  <key>CFBundleDisplayName</key>\n  <string>{}</string>\n",
           settings.bundle_name())?;
    write!(file,
           "  <key>CFBundleIdentifier</key>\n  <string>{}</string>\n",
           settings.identifier)?;
    write!(file,
           "  <key>CFBundleVersion</key>\n  <string>{}</string>\n",
           settings.version_string())?;
    if !icon_filenames.is_empty() {
        write!(file, "  <key>CFBundleVersion</key>\n  <array>\n")?;
        for filename in icon_filenames {
            write!(file, "    <string>{}</string>\n", filename)?;
        }
        write!(file, "  </array>\n")?;
    }
    // Note that this key is true for all iOS apps, even non-iPhone ones.
    write!(file, "  <key>LSRequiresIPhoneOS</key>\n  <true/>\n")?;
    write!(file, "</dict>\n</plist>\n")?;
    file.flush()?;
    Ok(())
}
