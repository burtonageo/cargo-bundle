// The structure of a Debian package looks something like this:
//
// foobar_1.2.3_i386.deb   # Actually a tar file
//     debian-binary           # Specifies deb format version (2.0 in our case)
//     control.tar.gz          # Contains files controlling the installation:
//         control                  # Basic package metadata
//         md5sums                  # Checksums for files in data.tar.gz below
//         postinst                 # Post-installation script (optional)
//         prerm                    # Pre-uninstallation script (optional)
//     data.tar.gz             # Contains files to be installed:
//         usr/bin/foobar                            # Binary executable file
//         usr/share/applications/foobar.desktop     # Desktop file (for apps)
//         usr/lib/foobar/...                        # Other resource files
//
// For cargo-bundle, we put bundle resource files under /usr/lib/package_name/,
// and then generate the desktop file and control file from the bundle
// metadata, as well as generating the md5sums file.  Currently we do not
// generate postinst or prerm files.

use {CargoSettings, Settings};
use libflate::gzip;
use md5;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use tar;
use walkdir::WalkDir;

pub fn bundle_project(settings: &Settings) -> ::Result<Vec<PathBuf>> {
    fn get_homepage(settings: &CargoSettings) -> &str {
        if !settings.description.is_empty() {
            &settings.description
        } else if !settings.homepage.is_empty() {
            &settings.homepage
        } else {
            &""
        }
    }

    let bin_file_metadata = {
        let bin_file = File::open(&settings.cargo_settings.binary_file)?;
        bin_file.metadata()?
    };
    let arch = env::consts::ARCH; // TODO(burtonageo): Use binary arch rather than host arch

    let package_dir = {
        let bin_name = settings.cargo_settings.binary_name()?;
        settings.cargo_settings
            .project_out_directory
            .join(format!("{}_{}_{}", bin_name, settings.version_string(), arch))
    };

    // Generate data files.
    let data_dir = package_dir.join("data");
    copy_file_to_dir(&settings.cargo_settings.binary_file,
                     data_dir.join("usr/bin"))?;
    transfer_resource_files(settings, &data_dir)?;
    generate_desktop_file(settings, &data_dir)?;
    // TODO: Generate icon file(s)

    // Generate control files.
    let control_dir = package_dir.join("control");
    let control_file_contents = format!("Package: {}\n\
                                         Version: {}\n\
                                         Architecture: {}\n\
                                         Maintainer: {}\n\
                                         Installed-Size: {}\n\
                                         Depends: {}\n\
                                         Suggests: {}\n\
                                         Conflicts: {}\n\
                                         Breaks: {}\n\
                                         Replaces: {}\n\
                                         Provides: {}\n\
                                         Section: {}\n\
                                         Priority: {}\n\
                                         Homepage: {}\n\
                                         Description: {}",
                                        settings.bundle_name,
                                        settings.cargo_settings.version,
                                        arch,
                                        settings.cargo_settings.authors.iter().fold(String::new(), |mut acc, s| {
                                            acc.push_str(&s);
                                            acc
                                        }),
                                        bin_file_metadata.len(), // TODO(burtonageo): Compute data size
                                        "deps",
                                        "suggests",
                                        "conflicts",
                                        "breaks",
                                        "replaces",
                                        "provides",
                                        "section",
                                        "priority",
                                        get_homepage(&settings.cargo_settings),
                                        settings.cargo_settings.description);
    create_file_with_data(&control_dir.join("control"), &control_file_contents)?;
    generate_md5sums(&control_dir, &data_dir)?;

    // Generate `debian-binary` file; see
    // http://www.tldp.org/HOWTO/Debian-Binary-Package-Building-HOWTO/x60.html#AEN66
    create_file_with_data(package_dir.join("debian-binary"), "2.0\n")?;

    // Apply tar/gzip to create the final package file.
    tar_and_gzip_dir(control_dir)?;
    tar_and_gzip_dir(data_dir)?;
    let deb_package_path = tar_dir_as_deb(package_dir)?;
    Ok(vec![deb_package_path])
}

/// Generate the application desktop file and store it under the `data_dir`.
fn generate_desktop_file(settings: &Settings, data_dir: &Path) -> ::Result<()> {
    let bin_name = settings.cargo_settings.binary_name()?;
    // For more information about the format of this file, see
    // https://developer.gnome.org/integration-guide/stable/desktop-files.html.en
    let desktop_file_contents = format!("[Desktop Entry]\n\
                                         Encoding=UTF-8\n\
                                         Exec={}\n\
                                         Icon={}\n\
                                         Name={}\n\
                                         Terminal=false\n\
                                         Type=Application\n\
                                         Version={}\n",
                                        bin_name,
                                        bin_name,
                                        settings.bundle_name,
                                        settings.version_string());
    let desktop_file_name = format!("{}.desktop", bin_name);
    let desktop_file_path = data_dir.join("usr/share/applications")
        .join(desktop_file_name);
    create_file_with_data(desktop_file_path, &desktop_file_contents)?;
    Ok(())
}

/// Create an `md5sums` file in the `control_dir` containing the MD5 checksums
/// for each file within the `data_dir`.
fn generate_md5sums(control_dir: &Path, data_dir: &Path) -> io::Result<()> {
    let md5sums_path = control_dir.join("md5sums");
    let mut md5sums_file = create_empty_file(&md5sums_path)?;
    for entry in WalkDir::new(data_dir) {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let mut file = File::open(path)?;
        let mut hash = md5::Context::new();
        io::copy(&mut file, &mut hash)?;
        for byte in hash.compute().iter() {
            write!(md5sums_file, "{:02x}", byte)?;
        }
        let rel_path = path.strip_prefix(data_dir).unwrap();
        let path_str = rel_path.to_str()
            .ok_or_else(|| {
                let msg = format!("Non-UTF-8 path: {:?}", rel_path);
                io::Error::new(io::ErrorKind::InvalidData, msg)
            })?;
        write!(md5sums_file, "  {}\n", path_str)?;
    }
    Ok(())
}

/// Copy the bundle's resource files into an appropriate directory under the
/// `data_dir`.
fn transfer_resource_files(settings: &Settings, data_dir: &Path) -> ::Result<()> {
    let bin_name = settings.cargo_settings.binary_name()?;
    let resource_dir = data_dir.join("usr/lib").join(bin_name);
    for res_path in &settings.resource_files {
        for entry in WalkDir::new(res_path) {
            let entry = entry?;
            let src_path = entry.path();
            if src_path.is_dir() {
                continue;
            }
            let dest_dir = if src_path.is_absolute() {
                resource_dir.clone()
            } else {
                resource_dir.join(src_path.parent()
                    .ok_or_else(|| {
                        let msg = format!("Not a file path: {:?}", src_path);
                        io::Error::new(io::ErrorKind::InvalidInput, msg)
                    })?)
            };
            copy_file_to_dir(src_path, dest_dir)?;
        }
    }
    Ok(())
}

/// Create an empty file at the given path, creating any parent directories as
/// needed.
fn create_empty_file<P: AsRef<Path>>(path: P) -> io::Result<BufWriter<File>> {
    let file_path = path.as_ref();
    let dir_path = file_path.parent()
        .ok_or_else(|| {
            let msg = format!("Not a file path: {:?}", file_path);
            io::Error::new(io::ErrorKind::InvalidInput, msg)
        })?;
    fs::create_dir_all(dir_path)?;
    let file = File::create(file_path)?;
    Ok(BufWriter::new(file))
}

/// Create an empty file at the given path, creating any parent directories as
/// needed, then write `data` into the file.
fn create_file_with_data<P: AsRef<Path>>(path: P, data: &str) -> io::Result<()> {
    let mut file = create_empty_file(path)?;
    file.write_all(data.as_bytes())?;
    file.flush()
}

/// Copy the file at the given path into the given directory, creating any
/// parent directories as needed.
fn copy_file_to_dir<P: AsRef<Path>, Q: AsRef<Path>>(file_path: P, dir_path: Q) -> io::Result<()> {
    let file_path = file_path.as_ref();
    let dir_path = dir_path.as_ref();
    let file_name = file_path.file_name()
        .ok_or_else(|| {
            let msg = format!("Not a file path: {:?}", file_path);
            io::Error::new(io::ErrorKind::InvalidInput, msg)
        })?;
    fs::create_dir_all(dir_path)?;
    fs::copy(file_path, dir_path.join(file_name))?;
    Ok(())
}

/// Writes a tar file to the given writer containing the given directory.
fn create_tar_from_dir<P: AsRef<Path>, W: Write>(src_dir: P, dest_file: W) -> io::Result<W> {
    let src_dir = src_dir.as_ref();
    println!("FIXME create_tar_from_dir {:?}", src_dir);
    let base_name = src_dir.file_name()
        .map(PathBuf::from)
        .ok_or_else(|| {
            let msg = format!("Directory has no name: {:?}", src_dir);
            io::Error::new(io::ErrorKind::InvalidInput, msg)
        })?;
    let mut tar_builder = tar::Builder::new(dest_file);
    for entry in WalkDir::new(&src_dir) {
        let entry = entry?;
        let src_path = entry.path();
        println!("FIXME entry {:?}", src_path);
        let src_path_rel = src_path.strip_prefix(&src_dir).unwrap();
        let dest_path = base_name.join(src_path_rel);
        if entry.file_type().is_dir() {
            tar_builder.append_dir(dest_path, src_path)?;
        } else {
            let mut src_file = fs::File::open(src_path)?;
            tar_builder.append_file(dest_path, &mut src_file)?;
        }
    }
    tar_builder.into_inner()
}

/// Creates a `.tar.gz` file from the given directory (placing the new file
/// within the given directory's parent directory), then deletes the original
/// directory and returns the path to the new file.
fn tar_and_gzip_dir<P: AsRef<Path>>(src_dir: P) -> io::Result<PathBuf> {
    let src_dir = src_dir.as_ref();
    println!("FIXME tar_and_gzip_dir {:?}", src_dir);
    let dest_path = src_dir.with_extension("tar.gz");
    let dest_file = create_empty_file(&dest_path)?;
    let gzip_encoder = gzip::Encoder::new(dest_file)?;
    let gzip_encoder = create_tar_from_dir(src_dir, gzip_encoder)?;
    let mut dest_file = gzip_encoder.finish().into_result()?;
    dest_file.flush()?;
    fs::remove_dir_all(src_dir)?;
    Ok(dest_path)
}

/// Creates a `.deb` file from the given directory (placing the new file within
/// the given directory's parent directory), then deletes the original
/// directory and returns the path to the new file.
fn tar_dir_as_deb<P: AsRef<Path>>(src_dir: P) -> io::Result<PathBuf> {
    let src_dir = src_dir.as_ref();
    println!("FIXME tar_dir_as_deb {:?}", src_dir);
    let dest_path = {
        let mut ext = src_dir.extension().unwrap_or(OsStr::new("")).to_os_string();
        ext.push(".deb");
        src_dir.with_extension(ext)
    };
    let dest_file = create_empty_file(&dest_path)?;
    let mut dest_file = create_tar_from_dir(src_dir, dest_file)?;
    dest_file.flush()?;
    fs::remove_dir_all(src_dir)?;
    Ok(dest_path)
}
