use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
    process::Command,
};

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
    let package_path = base_dir.join(&package_name);

    let app_dir = package_dir.join("AppDir");
    let binary_dest = app_dir.join("usr/bin").join(settings.binary_name());
    common::copy_file(settings.binary_path(), &binary_dest)?;
    generate_icon_files(settings, &app_dir)?;
    generate_desktop_file(settings, &app_dir)?;

    // TODO Symlinks (AppRun, .DirIcon, .desktop)
    common::symlink_file(&binary_dest, &app_dir.join("AppRun"))?;

    // Download the AppImage runtime
    let runtime = fetch_runtime(settings.binary_arch())?;

    // Make the squashfs
    let squashfs = base_dir.join(format!("{}.squashfs", package_name));
    let _status = Command::new("mksquashfs")
        .arg(&app_dir)
        .arg(&squashfs)
        .arg("-root-owned")
        .arg("-noappend")
        .arg("-quiet")
        .status()
        .chain_err(|| "Failed to make sqaushfs")?;

    // Write the runtime and the fs to the .AppImage file
    let mut squashfs = BufReader::new(File::open(squashfs)?);
    let mut f = File::create(&package_path)?;
    let mut out = BufWriter::new(&mut f);
    out.write_all(&runtime)?;
    std::io::copy(&mut squashfs, &mut out)?;

    Ok(vec![package_path])
}

fn fetch_runtime(arch: &str) -> crate::Result<Vec<u8>> {
    let url = format!(
        "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-{}",
        arch
    );

    let response = reqwest::blocking::get(url)
        .chain_err(|| "Failed to get appimage runtime")?
        .bytes()
        .chain_err(|| "Failed to ready bytes")?;

    Ok(response.to_vec())
}
