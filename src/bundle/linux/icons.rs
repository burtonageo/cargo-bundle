//! FreeDesktop hicolor icon theme installation for Linux packages.

use crate::bundle::{Settings, common};
use image::GenericImageView;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};

/// A rendered icon's pixel dimensions plus whether it is a `@2x` high-density
/// variant. Used to install each distinct size only once.
type IconSize = (u32, u32, bool);

/// Path of a hicolor themed PNG for one size, e.g.
/// `<base>/128x128@2x/apps/<binary>.png`.
fn hicolor_png_path(
    base_directory: &Path,
    binary_name: &str,
    (width, height, high_density): IconSize,
) -> PathBuf {
    let density_suffix = if high_density { "@2x" } else { "" };
    base_directory.join(format!(
        "{width}x{height}{density_suffix}/apps/{binary_name}.png"
    ))
}

/// Copy a source PNG into the theme, keyed by its dimensions so each size is
/// installed only once.
fn install_png_icon(
    icon_path: &Path,
    base_directory: &Path,
    binary_name: &str,
    seen_sizes: &mut BTreeSet<IconSize>,
) -> crate::Result<()> {
    let image = image::ImageReader::open(icon_path)?
        .with_guessed_format()?
        .decode()?;
    let (width, height) = image.dimensions();
    let size = (width, height, common::is_retina(icon_path));

    if seen_sizes.insert(size) {
        common::copy_file(
            icon_path,
            &hicolor_png_path(base_directory, binary_name, size),
        )?;
    }
    Ok(())
}

/// Install an ICNS family (one PNG per contained size) or any other raster
/// format (re-encoded to a single PNG).
fn install_other_icon(
    icon_path: &Path,
    base_directory: &Path,
    binary_name: &str,
    seen_sizes: &mut BTreeSet<IconSize>,
) -> crate::Result<()> {
    if icon_path.extension() == Some(OsStr::new("icns")) {
        let icon_family = icns::IconFamily::read(File::open(icon_path)?)?;
        for icon_type in icon_family.available_icons() {
            let size = (
                icon_type.screen_width(),
                icon_type.screen_height(),
                icon_type.pixel_density() > 1,
            );
            if seen_sizes.insert(size) {
                let icon = icon_family.get_icon_with_type(icon_type)?;
                let destination = hicolor_png_path(base_directory, binary_name, size);
                icon.write_png(common::create_file(&destination)?)?;
            }
        }
    } else {
        let icon = image::open(icon_path)?;
        let (width, height) = icon.dimensions();
        let size = (width, height, common::is_retina(icon_path));
        if seen_sizes.insert(size) {
            let destination = hicolor_png_path(base_directory, binary_name, size);
            icon.write_to(
                &mut common::create_file(&destination)?,
                image::ImageFormat::Png,
            )?;
        }
    }
    Ok(())
}

/// Generate the icon files and store them under the `data_dir`.
pub fn generate_icon_files(settings: &Settings, data_dir: &Path) -> crate::Result<()> {
    let base_directory = data_dir.join("usr/share/icons/hicolor");
    let binary_name = settings.binary_name();
    let mut seen_sizes = BTreeSet::new();

    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        match icon_path.extension().and_then(OsStr::to_str) {
            Some("svg") => {
                let scalable_directory = base_directory.join("scalable/apps");
                std::fs::create_dir_all(&scalable_directory)?;
                let destination = scalable_directory.join(format!("{binary_name}.svg"));
                common::copy_file(&icon_path, &destination)?;
            }
            Some("png") => {
                install_png_icon(&icon_path, &base_directory, binary_name, &mut seen_sizes)?;
            }
            _ => install_other_icon(&icon_path, &base_directory, binary_name, &mut seen_sizes)?,
        }
    }
    Ok(())
}
