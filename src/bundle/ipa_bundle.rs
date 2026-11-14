// An IPA is a ZIP archive that holds a signed app inside a `Payload` directory:
//
// Foobar.ipa
//     Payload/
//         Foobar.app/    # Exactly what the `ios` bundle produces
//
// Entries are written in sorted order with fixed timestamps and permissions,
// so identical inputs always produce byte-identical archives.

use super::{common, ios_bundle};
use crate::Settings;
use anyhow::{Context, bail, ensure};
use std::fs::{self, File};
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    // Simulator and Mac Catalyst builds cannot be installed from an IPA.
    ensure!(
        settings.target_triples().eq(["aarch64-apple-ios"]),
        "IPA packaging requires an iOS device target; pass --target aarch64-apple-ios"
    );
    let staging = tempfile::tempdir().context("Failed to create IPA staging directory")?;
    let payload = staging.path().join("Payload");
    let app = ios_bundle::bundle_project_at(settings, &payload)?;

    let directory = settings.project_out_directory().join("bundle/ipa");
    fs::create_dir_all(&directory)
        .with_context(|| format!("Failed to create bundle directory {directory:?}"))?;
    let ipa = directory.join(format!("{}.ipa", settings.bundle_name()));
    common::print_bundling(&format!("{}.ipa", settings.bundle_name()))?;
    let file = File::create(&ipa).with_context(|| format!("Failed to create {ipa:?}"))?;
    write_archive(file, &payload, &app.join(settings.binary_name()))?;
    Ok(vec![ipa])
}

/// Archive `payload` (named `Payload` on disk) with `executable` marked executable.
fn write_archive(
    writer: impl Write + Seek,
    payload: &Path,
    executable: &Path,
) -> crate::Result<()> {
    let root = payload.parent().context("IPA payload has no parent")?;
    let options = |mode| {
        SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(DateTime::default())
            .unix_permissions(mode)
    };
    let mut zip = ZipWriter::new(writer);
    for entry in walkdir::WalkDir::new(payload).sort_by_file_name() {
        let entry = entry?;
        let path = entry.path();
        let name = common::archive_path(path.strip_prefix(root)?)?;
        if entry.file_type().is_dir() {
            zip.add_directory(name, options(0o755))?;
        } else if entry.file_type().is_file() {
            zip.start_file(
                name,
                options(if path == executable { 0o755 } else { 0o644 }),
            )?;
            io::copy(&mut File::open(path)?, &mut zip)?;
        } else {
            // Links would need target validation to stay inside the app.
            bail!("IPA cannot contain {path:?}: only files and directories are supported");
        }
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    fn app(root: &Path) -> (PathBuf, PathBuf) {
        let app = root.join("Payload/Foo.app");
        fs::create_dir_all(app.join("res")).unwrap();
        fs::write(app.join("foo"), "binary").unwrap();
        fs::write(app.join("Info.plist"), "plist").unwrap();
        fs::write(app.join("res/data.txt"), "data").unwrap();
        (root.join("Payload"), app.join("foo"))
    }

    #[test]
    fn archives_the_payload_deterministically_with_fixed_modes() {
        let temp = tempfile::tempdir().unwrap();
        let (payload, executable) = app(temp.path());
        let write = || {
            let mut bytes = Cursor::new(Vec::new());
            write_archive(&mut bytes, &payload, &executable).unwrap();
            bytes.into_inner()
        };
        let bytes = write();
        assert_eq!(bytes, write());

        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut entries = Vec::new();
        for index in 0..zip.len() {
            let mut file = zip.by_index(index).unwrap();
            let mut content = String::new();
            file.read_to_string(&mut content).unwrap_or_default();
            entries.push((file.name().to_owned(), file.unix_mode().unwrap(), content));
        }
        let entry = |name: &str, mode, content: &str| (name.to_owned(), mode, content.to_owned());
        assert_eq!(
            entries,
            [
                entry("Payload/", 0o40755, ""),
                entry("Payload/Foo.app/", 0o40755, ""),
                entry("Payload/Foo.app/Info.plist", 0o100644, "plist"),
                entry("Payload/Foo.app/foo", 0o100755, "binary"),
                entry("Payload/Foo.app/res/", 0o40755, ""),
                entry("Payload/Foo.app/res/data.txt", 0o100644, "data"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let (payload, executable) = app(temp.path());
        std::os::unix::fs::symlink("/etc/passwd", payload.join("Foo.app/link")).unwrap();
        let error = write_archive(Cursor::new(Vec::new()), &payload, &executable).unwrap_err();
        assert!(error.to_string().contains("link"), "{error}");
    }
}
