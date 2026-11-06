// A DMG is a compressed HFS+ disk image holding the application bundle, an
// `Applications` link to drag it onto, and optional Finder presentation files:
//
//   <AppName>.dmg
//     <AppName>.app
//     Applications -> /Applications
//     .background/background.tiff, .DS_Store   (with `dmg_background`)
//
// The image is written in pure Rust, so it can be built on any host.

use super::{common, dmg_finder, hfs_image, osx_bundle, signing};
use crate::Settings;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let disk_image_name = format!("{}.dmg", settings.bundle_name());
    common::print_bundling(&disk_image_name)?;

    let directory = settings.project_out_directory().join("bundle/dmg");
    fs::create_dir_all(&directory)
        .with_context(|| format!("Failed to create bundle directory {directory:?}"))?;
    let disk_image_path = directory.join(&disk_image_name);

    // Everything is built in scratch space, and the image is only moved into
    // place once it is complete and signed, so a failure leaves any earlier
    // image alone.
    let staging = tempfile::tempdir().context("Failed to create the staging directory")?;
    let volume_tree = staging.path().join("volume");
    fs::create_dir(&volume_tree)?;
    osx_bundle::bundle_project_at(settings, &volume_tree)
        .context("Failed to create the application bundle for the disk image")?;
    let application_name = format!("{}.app", settings.bundle_name());
    let decoration = dmg_finder::prepare(settings, &volume_tree)?;

    let candidate = tempfile::NamedTempFile::new_in(&directory)
        .context("Failed to create the disk image")?
        .into_temp_path();
    hfs_image::write_image(
        &volume_tree,
        &candidate,
        settings.bundle_name(),
        |volume, device| {
            volume.create_symlink(device, "/Applications", "/Applications", 0o755, 0, 0, 0)?;
            if let Some(decoration) = &decoration {
                let ds_store = dmg_finder::ds_store(&application_name, decoration, volume)?;
                let length = ds_store.len() as u64;
                volume.create_file(
                    device,
                    "/.DS_Store",
                    &mut ds_store.as_slice(),
                    length,
                    0o644,
                    0,
                    0,
                    0,
                )?;
            }
            Ok(())
        },
    )
    .context("Failed to build the HFS+ disk image")?;

    signing::sign_apple_path(settings, &candidate)?;
    candidate
        .persist(&disk_image_path)
        .with_context(|| format!("Failed to write {disk_image_path:?}"))?;
    Ok(vec![disk_image_path])
}
