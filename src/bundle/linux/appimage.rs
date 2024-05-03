use std::path::PathBuf;

use crate::{
    bundle::{common, Settings},
    ResultExt,
};

use super::common::{generate_desktop_file, generate_icon_files};

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let package_base_name = format!(
        "{}_{}_{}",
        settings.binary_name(),
        settings.version_string(),
        settings.binary_arch()
    );
    let package_name = format!("{package_base_name}.AppImage");
    common::print_bundling(&package_name)?;

    let base_dir = settings.project_out_directory().join("bundle/appimage");
    let package_dir = base_dir.join(&package_base_name);
    if package_dir.exists() {
        std::fs::remove_dir_all(&package_dir)
            .chain_err(|| format!("Failed to remove old {package_base_name}"))?;
    }
    let package_path = base_dir.join(package_name);

    let data_dir = package_dir.join("AppDir");
    let binary_dest = data_dir.join("usr/bin").join(settings.binary_name());
    common::copy_file(settings.binary_path(), &binary_dest)?;
    generate_icon_files(settings, &data_dir)?;
    generate_desktop_file(settings, &data_dir)?;

    // TODO Symlinks

    // TODO Generate .AppImage (either call linuxdeploy, or find a crate to generate it)

    Ok(vec![package_path])
}
