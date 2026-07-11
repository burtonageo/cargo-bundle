// A macOS DMG (disk image) bundle is a compressed disk image that contains the
// application bundle and a symlink to /Applications so the user can simply
// drag-and-drop to install.
//
// The layout inside the mounted volume is:
//
//   <AppName>.dmg  (read-only compressed UDZO image)
//     <AppName>.app   # the application bundle
//     Applications -> /Applications  # drag-and-drop install target
//
// Building requires macOS because the `hdiutil` command is used to create and
// convert the disk image.

use super::common;
use crate::Settings;
use crate::bundle::osx_bundle;
use anyhow::Context;
use core::str::from_utf8;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MINIMUM_BUNDLE_SIZE_BYTES: u64 = 52_428_800; // 50 MiB

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let disk_image_name = format!("{}.dmg", settings.bundle_name());
    common::print_bundling(&disk_image_name)?;

    let base_directory = prepare_base_directory(settings)?;

    let application_bundle_path = build_application_bundle(settings, &base_directory)?;
    let final_disk_image_path = base_directory.join(&disk_image_name);

    clear_existing_path(final_disk_image_path.as_path())?;

    let temporary_directory = tempfile::tempdir()
        .context("Failed to create temporary directory for staging disk image")?;
    let staging_disk_image_path = temporary_directory.path().join("staging.dmg");

    create_staging_disk_image(
        &application_bundle_path,
        &staging_disk_image_path,
        settings.bundle_name(),
    )?;
    populate_disk_image(settings, &staging_disk_image_path, &application_bundle_path)?;
    compress_disk_image(&staging_disk_image_path, &final_disk_image_path)?;

    Ok(vec![final_disk_image_path])
}

/// Sets up the base directory where the bundle will be staged.
fn prepare_base_directory(settings: &Settings) -> crate::Result<PathBuf> {
    let base_directory = settings.project_out_directory().join("bundle/dmg");
    fs::create_dir_all(&base_directory)
        .with_context(|| format!("Failed to create bundle directory {base_directory:?}"))?;
    Ok(base_directory)
}

/// Builds the .app bundle and ensures any old builds are cleared out first.
fn build_application_bundle(settings: &Settings, base_directory: &Path) -> crate::Result<PathBuf> {
    let application_bundle_name = format!("{}.app", settings.bundle_name());
    let application_bundle_path = base_directory.join(&application_bundle_name);

    clear_existing_path(application_bundle_path.as_path())?;

    osx_bundle::bundle_project_at(settings, base_directory)
        .context("Failed to create application bundle for disk image")?;

    Ok(application_bundle_path)
}

/// Provisions a writable HFS+ disk image with enough capacity for the app.
fn create_staging_disk_image(
    application_bundle_path: &Path,
    staging_disk_image_path: &Path,
    volume_name: &str,
) -> crate::Result<()> {
    let bundle_size_bytes = calculate_directory_size(application_bundle_path)?;
    let image_size_bytes = bundle_size_bytes + MINIMUM_BUNDLE_SIZE_BYTES;

    let mut command = Command::new("hdiutil");
    command
        .arg("create")
        .arg(staging_disk_image_path)
        .arg("-ov")
        .arg("-fs")
        .arg("HFS+")
        .arg("-size")
        .arg(image_size_bytes.to_string())
        .arg("-volname")
        .arg(volume_name);

    command
        .output()
        .with_context(|| format!("Failed to spawn process: {:?}", command.get_program()))?;

    Ok(())
}

/// Mounts the staging image, copies assets over, and guarantees unmounting via RAII Drop.
fn populate_disk_image(
    settings: &Settings,
    staging_disk_image_path: &Path,
    application_bundle_path: &Path,
) -> crate::Result<()> {
    let mount_guard = mount_disk_image(staging_disk_image_path)?;
    let mount_point = mount_guard.mount_point();

    let application_name = application_bundle_path
        .file_name()
        .context("Application bundle path is missing a file name")?;

    common::copy_dir(application_bundle_path, &mount_point.join(application_name))?;

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/Applications", mount_point.join("Applications"))
            .context("Failed to create /Applications symbolic link")?;

        super::dmg_layout::decorate_volume(settings, mount_point, application_name)?;
    }

    Ok(())
}

/// Converts the writable staging image into a read-only, compressed UDZO image.
fn compress_disk_image(
    staging_disk_image_path: &Path,
    final_disk_image_path: &Path,
) -> crate::Result<()> {
    let mut command = Command::new("hdiutil");
    command
        .arg("convert")
        .arg(staging_disk_image_path)
        .arg("-ov")
        .arg("-format")
        .arg("UDZO")
        .arg("-imagekey")
        .arg("zlib-level=9")
        .arg("-o")
        .arg(final_disk_image_path);

    command
        .output()
        .with_context(|| format!("Failed to spawn process: {:?}", command.get_program()))?;

    Ok(())
}

/// Utility guard that ensures the attached disk image is detached when dropped.
struct DiskImageMountGuard {
    mount_point: PathBuf,
}

impl DiskImageMountGuard {
    fn mount_point(&self) -> &Path {
        self.mount_point.as_path()
    }
}

impl Drop for DiskImageMountGuard {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .arg("detach")
            .arg(&self.mount_point)
            .status();
    }
}

/// Attaches the disk image and returns the RAII guard containing the mount path.
fn mount_disk_image(disk_image_path: &Path) -> crate::Result<DiskImageMountGuard> {
    let mut command = Command::new("hdiutil");
    command
        .arg("attach")
        .arg(disk_image_path)
        .arg("-nobrowse")
        .arg("-noverify")
        .arg("-noautoopen")
        .arg("-noautofsck");

    let output = command
        .output()
        .with_context(|| format!("Failed to spawn process: {:?}", command.get_program()))?;

    let mount_point = parse_mount_point(&output.stdout)?;

    Ok(DiskImageMountGuard { mount_point })
}

/// Parses standard output from hdiutil attach to locate the /Volumes/ mount path.
fn parse_mount_point(standard_output: &[u8]) -> crate::Result<PathBuf> {
    let text_output = from_utf8(standard_output)?;

    let mount_point = text_output.lines().rev().find_map(|line| {
        line.split('\t')
            .last()
            .map(str::trim)
            .filter(|path| path.starts_with("/Volumes/"))
    });

    match mount_point {
        Some(path) => Ok(PathBuf::from(path)),
        None => anyhow::bail!("Could not find a /Volumes/… mount point in hdiutil standard output"),
    }
}

/// Recursively sums the file sizes within a directory.
fn calculate_directory_size(directory: &Path) -> crate::Result<u64> {
    let mut total_size_bytes: u64 = 0;

    for entry in walkdir::WalkDir::new(directory) {
        let entry = entry?;
        if entry.file_type().is_file() {
            total_size_bytes += entry.metadata()?.len();
        }
    }

    Ok(total_size_bytes)
}

/// Safely removes a file or directory if it exists, abstracting away duplicate logic.
fn clear_existing_path(path: &Path) -> crate::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove existing directory at {path:?}"))
    } else {
        fs::remove_file(path).with_context(|| format!("Failed to remove existing file at {path:?}"))
    }
}
