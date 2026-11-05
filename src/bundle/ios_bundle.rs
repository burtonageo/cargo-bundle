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
use super::signing;
use crate::Settings;
use anyhow::Context;
use image::{self, GenericImageView};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

/// Pixel sizes of the icons rendered from an SVG: every size iPhone and iPad
/// ask for, from the 20pt notification icon to the 1024px App Store icon.
const SVG_ICON_SIZES: [u32; 13] = [20, 29, 40, 58, 60, 76, 80, 87, 120, 152, 167, 180, 1024];

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let base_dir = settings.project_out_directory().join("bundle/ios");
    Ok(vec![bundle_project_at(settings, &base_dir)?])
}

/// Build and sign `<bundle name>.app` inside `output_dir`.
pub fn bundle_project_at(settings: &Settings, output_dir: &Path) -> crate::Result<PathBuf> {
    common::print_warning("iOS bundle support is still experimental.")?;
    validate_bundle_identifier(&settings.bundle_identifier())?;

    let app_bundle_name = format!("{}.app", settings.bundle_name());
    common::print_bundling(&app_bundle_name)?;
    let bundle_dir = output_dir.join(&app_bundle_name);
    if bundle_dir.exists() {
        fs::remove_dir_all(&bundle_dir)
            .with_context(|| format!("Failed to remove old {app_bundle_name}"))?;
    }
    fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("Failed to create bundle directory at {bundle_dir:?}"))?;

    for src in settings.resource_files() {
        let src = src?;
        let dest = bundle_dir.join(common::resource_relpath(&src));
        common::copy_file(&src, &dest)
            .with_context(|| format!("Failed to copy resource file {src:?}"))?;
    }

    let icon_filenames =
        generate_icon_files(&bundle_dir, settings).with_context(|| "Failed to create app icons")?;
    write_info_plist(&bundle_dir, settings, &icon_filenames)
        .with_context(|| "Failed to create Info.plist")?;
    if let Some(localizations) = settings.ios_localizations() {
        localizations
            .write_to_directory(&bundle_dir)
            .with_context(|| "Failed to write iOS localizations")?;
    }
    let bin_path = bundle_dir.join(settings.binary_name());
    common::copy_file(settings.binary_path(), &bin_path)
        .with_context(|| format!("Failed to copy binary from {:?}", settings.binary_path()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755))?;
    }
    // Everything the signature should seal must be in place before this point.
    signing::embed_apple_provisioning_profile(
        settings,
        &bundle_dir.join("embedded.mobileprovision"),
    )?;
    signing::sign_apple_path(settings, &bundle_dir)?;
    Ok(bundle_dir)
}

/// An unset identifier is empty, and a device refuses to install such an app.
fn validate_bundle_identifier(identifier: &str) -> crate::Result<()> {
    let valid = identifier.split('.').all(|part| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    anyhow::ensure!(
        valid,
        "iOS needs a bundle identifier of dot-separated parts made of letters, digits and \
         hyphens, not {identifier:?}; set `identifier` in package.metadata.bundle"
    );
    Ok(())
}

/// Generate the icon files and store them under the `bundle_dir`.
fn generate_icon_files(bundle_dir: &Path, settings: &Settings) -> crate::Result<Vec<String>> {
    let icons = settings.icon_files().collect::<crate::Result<Vec<_>>>()?;

    // As on macOS, the first SVG is the source of truth for every size.
    if let Some(svg) = icons
        .iter()
        .find(|icon| icon.extension() == Some(OsStr::new("svg")))
    {
        let tree = common::load_svg(svg)?;
        let mut filenames = Vec::new();
        for size in SVG_ICON_SIZES {
            let filename = format!("icon_{size}x{size}.png");
            common::render_svg(&tree, size, size)?
                .save_png(bundle_dir.join(&filename))
                .with_context(|| format!("Failed to write {filename}"))?;
            filenames.push(filename);
        }
        return Ok(filenames);
    }

    let mut filenames = Vec::new();
    {
        let mut get_dest_path = |width: u32, height: u32, is_retina: bool| {
            let filename = format!(
                "icon_{}x{}{}.png",
                width,
                height,
                if is_retina { "@2x" } else { "" }
            );
            let path = bundle_dir.join(&filename);
            filenames.push(filename);
            path
        };
        let mut sizes = BTreeSet::new();
        // Prefer PNG files.
        for icon_path in &icons {
            if icon_path.extension() != Some(OsStr::new("png")) {
                continue;
            }
            let img = image::ImageReader::open(icon_path)?
                .with_guessed_format()?
                .decode()?;
            let (width, height) = img.dimensions();
            let is_retina = common::is_retina(icon_path);
            if !sizes.contains(&(width, height, is_retina)) {
                sizes.insert((width, height, is_retina));
                let dest_path = get_dest_path(width, height, is_retina);
                common::copy_file(icon_path, &dest_path)?;
            }
        }
        // Fall back to non-PNG files for any missing sizes.
        for icon_path in &icons {
            if icon_path.extension() == Some(OsStr::new("png")) {
                continue;
            } else if icon_path.extension() == Some(OsStr::new("icns")) {
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
                let icon = image::open(icon_path)?;
                let (width, height) = icon.dimensions();
                let is_retina = common::is_retina(icon_path);
                if !sizes.contains(&(width, height, is_retina)) {
                    sizes.insert((width, height, is_retina));
                    let dest_path = get_dest_path(width, height, is_retina);
                    let mut file = common::create_file(&dest_path)?;
                    icon.write_to(&mut file, image::ImageFormat::Png)?;
                }
            }
        }
    }
    Ok(filenames)
}

fn write_info_plist(
    bundle_dir: &Path,
    settings: &Settings,
    icon_filenames: &[String],
) -> crate::Result<()> {
    use plist::{Dictionary, Value};

    let version = settings.version_string().to_string();
    let mut info = Dictionary::new();
    info.insert(
        "CFBundleIdentifier".into(),
        settings.bundle_identifier().into_owned().into(),
    );
    info.insert("CFBundleDisplayName".into(), settings.bundle_name().into());
    info.insert("CFBundleName".into(), settings.bundle_name().into());
    info.insert("CFBundleExecutable".into(), settings.binary_name().into());
    info.insert("CFBundleVersion".into(), version.clone().into());
    info.insert("CFBundleShortVersionString".into(), version.into());
    info.insert("CFBundleDevelopmentRegion".into(), "en_US".into());
    info.insert("UILaunchStoryboardName".into(), "".into());
    if !icon_filenames.is_empty() {
        let icons = icon_filenames
            .iter()
            .map(|name| name.as_str().into())
            .collect();
        info.insert("CFBundleIconFiles".into(), Value::Array(icons));
    }
    info.insert("LSRequiresIPhoneOS".into(), true.into());
    Value::Dictionary(info)
        .to_file_xml(bundle_dir.join("Info.plist"))
        .context("Failed to write Info.plist")
}
