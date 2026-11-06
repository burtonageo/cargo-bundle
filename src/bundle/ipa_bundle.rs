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
use std::path::{Component, Path, PathBuf};
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
    publish(&ipa, &payload, &app.join(settings.binary_name()))?;
    Ok(vec![ipa])
}

/// Archives the payload beside `ipa` and moves it into place only once it is
/// complete, so a failure never leaves a truncated file or loses the old one.
fn publish(ipa: &Path, payload: &Path, executable: &Path) -> crate::Result<()> {
    let directory = ipa.parent().context("IPA path has no parent")?;
    let mut candidate = tempfile::NamedTempFile::new_in(directory)
        .with_context(|| format!("Failed to create a file in {directory:?}"))?;
    write_archive(candidate.as_file_mut(), payload, executable)?;
    candidate
        .persist(ipa)
        .with_context(|| format!("Failed to write {ipa:?}"))?;
    Ok(())
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
        } else if entry.file_type().is_symlink() {
            // Frameworks use links such as `Versions/Current`. They are kept as
            // links, never followed, and may not point out of the app.
            let target = fs::read_link(path)?;
            check_link(path.strip_prefix(payload)?, &target)?;
            let target = target
                .to_str()
                .context("IPA symlink targets must be UTF-8")?;
            zip.add_symlink(name, target, options(0o777))?;
        } else {
            bail!("IPA cannot contain {path:?}: only files, directories and links are supported");
        }
    }
    zip.finish()?;
    Ok(())
}

/// A link at `link` (relative to `Payload`, so starting with the app
/// directory) may only point at something inside that app directory.
fn check_link(link: &Path, target: &Path) -> crate::Result<()> {
    let mut depth = link
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 1 => depth -= 1,
            _ => bail!("IPA symlink {link:?} points outside the app: {target:?}"),
        }
    }
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
    fn links_stay_links_inside_the_app_and_escaping_ones_keep_the_previous_ipa() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let (payload, executable) = app(temp.path());
        let app = payload.join("Foo.app");
        symlink("foo", app.join("alias")).unwrap();
        symlink("../Info.plist", app.join("res/up")).unwrap();
        let mut bytes = Cursor::new(Vec::new());
        write_archive(&mut bytes, &payload, &executable).unwrap();
        let mut zip = zip::ZipArchive::new(bytes).unwrap();
        for (name, target) in [("alias", "foo"), ("res/up", "../Info.plist")] {
            let mut link = zip.by_name(&format!("Payload/Foo.app/{name}")).unwrap();
            assert_eq!(link.unix_mode().unwrap() & 0o170000, 0o120000, "{name}");
            let mut stored = String::new();
            link.read_to_string(&mut stored).unwrap();
            assert_eq!(stored, target);
        }
        drop(zip);
        fs::remove_file(app.join("alias")).unwrap();
        fs::remove_file(app.join("res/up")).unwrap();

        let ipa = temp.path().join("Foo.ipa");
        for target in ["/etc/passwd", "../../outside", "../../../x"] {
            fs::write(&ipa, "previous").unwrap();
            symlink(target, app.join("link")).unwrap();
            let error = publish(&ipa, &payload, &executable).unwrap_err();
            assert!(
                error.to_string().contains("outside the app"),
                "{target}: {error}"
            );
            assert_eq!(fs::read_to_string(&ipa).unwrap(), "previous");
            // No half-written file is left behind either.
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
            fs::remove_file(app.join("link")).unwrap();
        }
    }
}
