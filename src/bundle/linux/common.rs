use crate::bundle::{common, Settings};
use image::GenericImageView;
use libflate::gzip;
use md5::Digest;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Generate the application desktop file and store it under the `data_dir`.
pub fn generate_desktop_file(settings: &Settings, data_dir: &Path) -> crate::Result<()> {
    let bin_name = settings.binary_name();
    let desktop_file_name = format!("{bin_name}.desktop");
    let desktop_file_path = data_dir
        .join("usr/share/applications")
        .join(desktop_file_name);
    let file = &mut common::create_file(&desktop_file_path)?;
    let mime_types = settings
        .linux_mime_types()
        .iter()
        .fold("".to_owned(), |acc, s| format!("{acc}{s};"));
    // For more information about the format of this file, see
    // https://developer.gnome.org/integration-guide/stable/desktop-files.html.en
    writeln!(file, "[Desktop Entry]")?;
    writeln!(file, "Encoding=UTF-8")?;
    if let Some(category) = settings.app_category() {
        writeln!(file, "Categories={}", category.gnome_desktop_categories())?;
    }
    if !settings.short_description().is_empty() {
        writeln!(file, "Comment={}", settings.short_description())?;
    }
    let exec = match settings.linux_exec_args() {
        Some(args) => format!("{bin_name} {args}"),
        None => bin_name.to_owned(),
    };
    writeln!(file, "Exec={exec}")?;
    writeln!(file, "Icon={bin_name}")?;
    writeln!(file, "Name={}", settings.bundle_name())?;
    writeln!(
        file,
        "Terminal={}",
        settings.linux_use_terminal().unwrap_or(false)
    )?;
    writeln!(file, "Type=Application")?;
    writeln!(file, "MimeType={mime_types}")?;
    // The `Version` field is omitted on pupose. See `generate_control_file` for specifying
    // the application version.
    Ok(())
}

/// Creates a `.tar.gz` file from the given directory (placing the new file
/// within the given directory's parent directory), then deletes the original
/// directory and returns the path to the new file.
pub fn tar_and_gzip_dir<P: AsRef<Path>>(src_dir: P) -> crate::Result<PathBuf> {
    let src_dir = src_dir.as_ref();
    let dest_path = src_dir.with_extension("tar.gz");
    let dest_file = common::create_file(&dest_path)?;
    let gzip_encoder = gzip::Encoder::new(dest_file)?;
    let gzip_encoder = create_tar_from_dir(src_dir, gzip_encoder)?;
    let mut dest_file = gzip_encoder.finish().into_result()?;
    dest_file.flush()?;
    Ok(dest_path)
}

/// Writes a tar file to the given writer containing the given directory.
pub fn create_tar_from_dir<P: AsRef<Path>, W: Write>(src_dir: P, dest_file: W) -> crate::Result<W> {
    let src_dir = src_dir.as_ref();
    let mut tar_builder = tar::Builder::new(dest_file);
    for entry in WalkDir::new(src_dir) {
        let entry = entry?;
        let src_path = entry.path();
        if src_path == src_dir {
            continue;
        }
        let dest_path = src_path.strip_prefix(src_dir).unwrap();
        if entry.file_type().is_dir() {
            tar_builder.append_dir(dest_path, src_path)?;
        } else {
            let mut src_file = File::open(src_path)?;
            tar_builder.append_file(dest_path, &mut src_file)?;
        }
    }
    let dest_file = tar_builder.into_inner()?;
    Ok(dest_file)
}

/// Create an empty file at the given path, creating any parent directories as
/// needed, then write `data` into the file.
pub fn create_file_with_data<P: AsRef<Path>>(path: P, data: &str) -> crate::Result<()> {
    let mut file = common::create_file(path.as_ref())?;
    file.write_all(data.as_bytes())?;
    file.flush()?;
    Ok(())
}

/// Computes the total size, in bytes, of the given directory and all of its
/// contents.
pub fn total_dir_size(dir: &Path) -> crate::Result<u64> {
    let mut total: u64 = 0;
    for entry in WalkDir::new(dir) {
        total += entry?.metadata()?.len();
    }
    Ok(total)
}

fn get_dest_path<'a>(
    width: u32,
    height: u32,
    is_high_density: bool,
    base_dir: &'a Path,
    binary_name: &'a str,
) -> PathBuf {
    Path::join(
        base_dir,
        format!(
            "{}x{}{}/apps/{}.png",
            width,
            height,
            if is_high_density { "@2x" } else { "" },
            binary_name
        ),
    )
}

fn generate_icon_files_png(
    icon_path: &PathBuf,
    base_dir: &Path,
    binary_name: &str,
    mut sizes: BTreeSet<(u32, u32, bool)>,
) -> crate::Result<BTreeSet<(u32, u32, bool)>> {
    let img = image::ImageReader::open(icon_path)?
        .with_guessed_format()?
        .decode()?;
    let (width, height) = img.dimensions();
    let is_high_density = common::is_retina(icon_path);

    if !sizes.contains(&(width, height, is_high_density)) {
        sizes.insert((width, height, is_high_density));
        let dest_path = get_dest_path(width, height, is_high_density, base_dir, binary_name);
        common::copy_file(icon_path, &dest_path)?;
    }

    Ok(sizes.to_owned())
}

fn generate_icon_files_non_png(
    icon_path: &PathBuf,
    base_dir: &Path,
    binary_name: &str,
    mut sizes: BTreeSet<(u32, u32, bool)>,
) -> crate::Result<BTreeSet<(u32, u32, bool)>> {
    if icon_path.extension() == Some(OsStr::new("icns")) {
        let icon_family = icns::IconFamily::read(File::open(icon_path)?)?;
        for icon_type in icon_family.available_icons() {
            let width = icon_type.screen_width();
            let height = icon_type.screen_height();
            let is_high_density = icon_type.pixel_density() > 1;

            if !sizes.contains(&(width, height, is_high_density)) {
                sizes.insert((width, height, is_high_density));
                let icon = icon_family.get_icon_with_type(icon_type)?;
                let dest_path =
                    get_dest_path(width, height, is_high_density, base_dir, binary_name);
                icon.write_png(common::create_file(&dest_path)?)?;
            }
        }
    } else {
        let icon = image::open(icon_path)?;
        let (width, height) = icon.dimensions();
        let is_high_density = common::is_retina(icon_path);

        if !sizes.contains(&(width, height, is_high_density)) {
            sizes.insert((width, height, is_high_density));
            let dest_path = get_dest_path(width, height, is_high_density, base_dir, binary_name);
            let mut file = common::create_file(&dest_path)?;
            icon.write_to(&mut file, image::ImageFormat::Png)?;
        }
    }

    Ok(sizes.to_owned())
}

/// Generate the icon files and store them under the `data_dir`.
pub fn generate_icon_files(settings: &Settings, data_dir: &Path) -> crate::Result<()> {
    let base_dir = data_dir.join("usr/share/icons/hicolor");

    let mut sizes: BTreeSet<(u32, u32, bool)> = BTreeSet::new();

    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        if icon_path.extension() == Some(OsStr::new("png")) {
            let new_sizes = generate_icon_files_png(
                &icon_path,
                &base_dir,
                settings.binary_name(),
                sizes.clone(),
            )
            .unwrap();
            sizes.append(&mut new_sizes.to_owned())
        } else {
            let new_sizes = generate_icon_files_non_png(
                &icon_path,
                &base_dir,
                settings.binary_name(),
                sizes.clone(),
            )
            .unwrap();
            sizes.append(&mut new_sizes.to_owned())
        }
    }

    Ok(())
}

/// Compute the md5 hash of the given file.
pub fn generate_md5sum(file_path: &Path) -> crate::Result<Digest> {
    let mut file = File::open(file_path)?;
    let mut hash = md5::Context::new();
    io::copy(&mut file, &mut hash)?;
    Ok(hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_tar_and_gzip_dir() {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir(temp_dir.path().join("foo")).unwrap();
        File::create(temp_dir.path().join("foo/file1.txt")).unwrap();
        std::fs::create_dir_all(temp_dir.path().join("foo/subdir")).unwrap();
        File::create(temp_dir.path().join("foo/subdir/file2.txt"))
            .unwrap()
            .write_all(b"test")
            .unwrap();
        let tar_gz_file = tar_and_gzip_dir(temp_dir.path().join("foo"));
        assert!(tar_gz_file.is_ok());
        let tar_gz_file = tar_gz_file.unwrap();

        assert!(tar_gz_file.exists());
        assert!(tar_gz_file.metadata().unwrap().len() > 0);
    }

    #[test]
    fn test_create_file_with_data() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("foo.txt");
        assert!(create_file_with_data(&file_path, "test").is_ok());
        assert!(file_path.exists());
        assert_eq!(file_path.metadata().unwrap().len(), 4);
    }

    #[test]
    fn test_generate_md5sum() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("foo.txt");
        File::create(&file_path)
            .unwrap()
            .write_all(b"test")
            .unwrap();
        let md5_sums = generate_md5sum(file_path.as_path());
        assert!(md5_sums.is_ok());
        let mut md5_str = String::new();

        for b in md5_sums.unwrap().iter() {
            md5_str.push_str(&format!("{b:02x}"));
        }

        assert_eq!(md5_str, "098f6bcd4621d373cade4e832627b4f6".to_string());
    }
}
