use anyhow::Context;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    ffi::OsStr,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
    process::Command,
};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::bundle::{Settings, common};

use super::{DesktopFileOptions, generate_desktop_file, generate_icon_files};

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
            .with_context(|| format!("Failed to remove old {package_base_name}"))?;
    }
    let package_path = base_dir.join(&package_name);

    let app_dir = package_dir.join("AppDir");
    let binary_dest_rel = PathBuf::from("usr/bin").join(settings.binary_name());
    let binary_dest_abs = app_dir.join(binary_dest_rel.clone());
    common::copy_file(settings.binary_path(), &binary_dest_abs)?;
    generate_icon_files(settings, &app_dir)?;
    generate_desktop_file(settings, &app_dir, &DesktopFileOptions::default())?;

    common::symlink_file(&binary_dest_rel, &app_dir.join("AppRun"))?;

    // Symlink the .desktop file to the AppDir root.
    let desktop_filename = format!("{}.desktop", settings.binary_name());
    let desktop_rel = PathBuf::from("usr/share/applications").join(&desktop_filename);
    common::symlink_file(&desktop_rel, &app_dir.join(&desktop_filename))?;

    // Generate a .DirIcon PNG from the first SVG icon, or symlink the first
    // PNG icon so AppImage tools can pick it up.
    generate_dir_icon(settings, &app_dir)?;

    // Download the AppImage runtime
    let runtime = fetch_runtime(settings.binary_arch())?;

    // Make the squashfs
    let squashfs = base_dir.join(format!("{package_name}.squashfs"));
    let _status = Command::new("mksquashfs")
        .arg(&app_dir)
        .arg(&squashfs)
        .arg("-root-owned")
        .arg("-noappend")
        .arg("-quiet")
        .status()
        .with_context(|| "Failed to make sqaushfs, does the mksquashfs binary exist?")?;

    // Write the runtime and the fs to the .AppImage file
    let mut squashfs = BufReader::new(File::open(squashfs)?);
    let mut f = File::create(&package_path)?;
    let mut out = BufWriter::new(&mut f);
    out.write_all(&runtime)?;
    std::io::copy(&mut squashfs, &mut out)?;

    #[allow(unused_mut)]
    let mut perms = std::fs::metadata(&package_path)?.permissions();
    #[cfg(unix)]
    perms.set_mode(0o755);
    std::fs::set_permissions(&package_path, perms)?;

    Ok(vec![package_path])
}

fn generate_dir_icon(settings: &Settings, app_dir: &std::path::Path) -> crate::Result<()> {
    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        if icon_path.extension() == Some(OsStr::new("svg")) {
            let svg_data = std::fs::read_to_string(&icon_path)
                .with_context(|| format!("Failed to read SVG icon {icon_path:?}"))?;

            let opt = Options::default();
            let tree = Tree::from_data(svg_data.as_bytes(), &opt)
                .with_context(|| "Failed to parse SVG for .DirIcon")?;

            let size: u32 = 256;
            let mut pixmap =
                Pixmap::new(size, size).with_context(|| "Failed to create pixmap for .DirIcon")?;
            resvg::render(
                &tree,
                Transform::from_scale(
                    size as f32 / tree.size().width(),
                    size as f32 / tree.size().height(),
                ),
                &mut pixmap.as_mut(),
            );

            pixmap
                .save_png(app_dir.join(".DirIcon"))
                .with_context(|| "Failed to save .DirIcon PNG")?;

            // Also symlink the SVG into the AppDir root so tools that prefer
            // the vector version can find it.
            let svg_filename = format!("{}.svg", settings.binary_name());
            let svg_rel =
                PathBuf::from("usr/share/icons/hicolor/scalable/apps").join(&svg_filename);
            let _ = common::symlink_file(&svg_rel, &app_dir.join(&svg_filename));

            return Ok(());
        }
    }

    Ok(())
}

fn fetch_runtime(arch: &str) -> crate::Result<Vec<u8>> {
    let url = format!(
        "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-{arch}"
    );

    let response = reqwest::blocking::get(url)
        .with_context(|| "Failed to get appimage runtime")?
        .bytes()
        .with_context(|| "Failed to ready bytes")?;

    Ok(response.to_vec())
}
