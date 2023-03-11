// The structure of a Debian package looks something like this:
//
// foobar_1.2.3_i386.deb   # Actually an ar archive
//     debian-binary           # Specifies deb format version (2.0 in our case)
//     control.tar.gz          # Contains files controlling the installation:
//         control                  # Basic package metadata
//         md5sums                  # Checksums for files in data.tar.gz below
//         postinst                 # Post-installation script (optional)
//         prerm                    # Pre-uninstallation script (optional)
//     data.tar.gz             # Contains files to be installed:
//         usr/bin/foobar                            # Binary executable file
//         usr/share/applications/foobar.desktop     # Desktop file (for apps)
//         usr/share/icons/hicolor/...               # Icon files (for apps)
//         usr/lib/foobar/...                        # Other resource files
//
// For cargo-bundle, we put bundle resource files under /usr/lib/package_name/,
// and then generate the desktop file and control file from the bundle
// metadata, as well as generating the md5sums file.  Currently we do not
// generate postinst or prerm files.

use {ResultExt, Settings};
use ar;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use bundle::common;
use bundle::linux::common::{create_file_with_data, generate_desktop_file,
                            generate_icon_files, generate_md5sum, tar_and_gzip_dir, total_dir_size};

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let arch = match settings.binary_arch() {
        "x86" => "i386",
        "x86_64" => "amd64",
        "arm" => "armhf", // ARM64 is detected differently, armel isn't supported, so armhf is the only reasonable choice here.
        "aarch64" => "arm64",
        other => other,
    };
    let package_base_name = format!(
        "{}_{}_{}",
        settings.binary_name(),
        settings.version_string(),
        arch
    );
    let package_name = format!("{package_base_name}.deb");
    common::print_bundling(&package_name)?;
    let base_dir = settings.project_out_directory().join("bundle/deb");
    let package_dir = base_dir.join(&package_base_name);
    if package_dir.exists() {
        std::fs::remove_dir_all(&package_dir).chain_err(|| {
            format!("Failed to remove old {package_base_name}")
        })?;
    }
    let package_path = base_dir.join(package_name);

    // Generate data files.
    let data_dir = package_dir.join("data");
    let binary_dest = data_dir.join("usr/bin").join(settings.binary_name());
    common::copy_file(settings.binary_path(), &binary_dest)
        .chain_err(|| "Failed to copy binary file")?;
    transfer_resource_files(settings, &data_dir).chain_err(|| "Failed to copy resource files")?;
    generate_icon_files(settings, &data_dir).chain_err(|| "Failed to create icon files")?;
    generate_desktop_file(settings, &data_dir).chain_err(|| "Failed to create desktop file")?;

    // Generate control files.
    let control_dir = package_dir.join("control");
    generate_control_file(settings, arch, &control_dir, &data_dir)
        .chain_err(|| "Failed to create control file")?;
    generate_md5sums(&control_dir, &data_dir).chain_err(|| "Failed to create md5sums file")?;

    // Generate `debian-binary` file; see
    // http://www.tldp.org/HOWTO/Debian-Binary-Package-Building-HOWTO/x60.html#AEN66
    let debian_binary_path = package_dir.join("debian-binary");
    create_file_with_data(&debian_binary_path, "2.0\n")
        .chain_err(|| "Failed to create debian-binary file")?;

    // Apply tar/gzip/ar to create the final package file.
    let control_tar_gz_path =
        tar_and_gzip_dir(control_dir).chain_err(|| "Failed to tar/gzip control directory")?;
    let data_tar_gz_path =
        tar_and_gzip_dir(data_dir).chain_err(|| "Failed to tar/gzip data directory")?;
    create_archive(
        vec![debian_binary_path, control_tar_gz_path, data_tar_gz_path],
        &package_path,
    )
    .chain_err(|| "Failed to create package archive")?;
    Ok(vec![package_path])
}

fn generate_control_file(
    settings: &Settings,
    arch: &str,
    control_dir: &Path,
    data_dir: &Path,
) -> crate::Result<()> {
    // For more information about the format of this file, see
    // https://www.debian.org/doc/debian-policy/ch-controlfields.html
    let dest_path = control_dir.join("control");
    let mut file = common::create_file(&dest_path)?;
    writeln!(
        &mut file,
        "Package: {}",
        str::replace(settings.bundle_name(), " ", "-").to_ascii_lowercase()
    )?;
    writeln!(&mut file, "Version: {}", settings.version_string())?;
    writeln!(&mut file, "Architecture: {arch}")?;
    writeln!(&mut file, "Installed-Size: {}", total_dir_size(data_dir)?)?;
    let authors = settings.authors_comma_separated().unwrap_or_default();
    writeln!(&mut file, "Maintainer: {authors}")?;
    if !settings.homepage_url().is_empty() {
        writeln!(&mut file, "Homepage: {}", settings.homepage_url())?;
    }
    let dependencies = settings.debian_dependencies();
    if !dependencies.is_empty() {
        writeln!(&mut file, "Depends: {}", dependencies.join(", "))?;
    }
    let mut short_description = settings.short_description().trim();
    if short_description.is_empty() {
        short_description = "(none)";
    }
    let mut long_description = settings.long_description().unwrap_or("").trim();
    if long_description.is_empty() {
        long_description = "(none)";
    }
    writeln!(&mut file, "Description: {short_description}")?;
    for line in long_description.lines() {
        let line = line.trim();
        if line.is_empty() {
            writeln!(&mut file, " .")?;
        } else {
            writeln!(&mut file, " {line}")?;
        }
    }
    file.flush()?;
    Ok(())
}

/// Create an `md5sums` file in the `control_dir` containing the MD5 checksums
/// for each file within the `data_dir`.
fn generate_md5sums(control_dir: &Path, data_dir: &Path) -> crate::Result<()> {
    let md5sums_path = control_dir.join("md5sums");
    let mut md5sums_file = common::create_file(&md5sums_path)?;
    for entry in WalkDir::new(data_dir) {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        for byte in generate_md5sum(path)?.iter() {
            write!(md5sums_file, "{byte:02x}")?;
        }
        let rel_path = path.strip_prefix(data_dir).unwrap();
        let path_str = rel_path.to_str().ok_or_else(|| {
            let msg = format!("Non-UTF-8 path: {rel_path:?}");
            io::Error::new(io::ErrorKind::InvalidData, msg)
        })?;
        writeln!(md5sums_file, "  {path_str}")?;
    }
    Ok(())
}

/// Copy the bundle's resource files into an appropriate directory under the
/// `data_dir`.
fn transfer_resource_files(settings: &Settings, data_dir: &Path) -> crate::Result<()> {
    let resource_dir = data_dir.join("usr/lib").join(settings.binary_name());
    for src in settings.resource_files() {
        let src = src?;
        let dest = resource_dir.join(common::resource_relpath(&src));
        common::copy_file(&src, &dest)
            .chain_err(|| format!("Failed to copy resource file {src:?}"))?;
    }
    Ok(())
}

/// Creates an `ar` archive from the given source files and writes it to the
/// given destination path.
fn create_archive(srcs: Vec<PathBuf>, dest: &Path) -> crate::Result<()> {
    let mut builder = ar::Builder::new(common::create_file(dest)?);
    for path in &srcs {
        builder.append_path(path)?;
    }
    builder.into_inner()?.flush()?;
    Ok(())
}
