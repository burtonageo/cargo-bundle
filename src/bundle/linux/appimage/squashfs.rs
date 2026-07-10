//! SquashFS payload creation and type-2 AppImage assembly.

use crate::bundle::Settings;
use anyhow::Context;
use backhand::{
    FilesystemCompressor, FilesystemWriter, NodeHeader,
    compression::Compressor,
    kind::{self, Kind},
};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use walkdir::WalkDir;

/// Resolve and validate the compression codec from settings. Returns `"gzip"` by
/// default; errors on unknown values.
fn resolve_compressor(settings: &Settings) -> crate::Result<Compressor> {
    match settings.appimage_compression().unwrap_or("gzip") {
        "gzip" => Ok(Compressor::Gzip),
        "zstd" => Ok(Compressor::Zstd),
        other => anyhow::bail!(
            "appimage_compression must be \"gzip\" or \"zstd\", got {:?}",
            other
        ),
    }
}

pub(super) fn create_and_assemble(
    settings: &Settings,
    app_directory_path: &Path,
    runtime_bytes: &[u8],
    package_path: &Path,
) -> crate::Result<()> {
    let temporary_directory =
        tempfile::tempdir().with_context(|| "Failed to create temporary directory for squashfs")?;
    let squash_filesystem_path = temporary_directory.path().join("payload.squashfs");

    let compressor = resolve_compressor(settings)?;

    // Build the payload, then prepend the runtime. On any failure, remove the
    // half-written AppImage so a retry starts clean.
    let result = build_via_backhand(app_directory_path, &squash_filesystem_path, compressor)
        .and_then(|()| concatenate_payloads(runtime_bytes, &squash_filesystem_path, package_path));

    if result.is_err() {
        let _ = fs::remove_file(package_path);
    }
    result
}

fn build_via_backhand(
    app_directory: &Path,
    output_path: &Path,
    compressor: Compressor,
) -> crate::Result<()> {
    let mut filesystem_writer = FilesystemWriter::default();
    filesystem_writer.set_current_time();
    filesystem_writer.set_only_root_id();
    filesystem_writer.set_no_padding();
    filesystem_writer.set_kind(Kind::from_const(kind::LE_V4_0).expect("LE_V4_0 kind"));

    let compressor = FilesystemCompressor::new(compressor, None)
        .context("Failed to configure squashfs compressor")?;
    filesystem_writer.set_compressor(compressor);

    populate_filesystem_writer(&mut filesystem_writer, app_directory)?;

    let mut output_file = File::create(output_path)
        .with_context(|| format!("Failed to create {}", output_path.display()))?;

    filesystem_writer
        .write(&mut output_file)
        .context("backhand failed to write squashfs")?;

    output_file.flush()?;
    Ok(())
}

fn populate_filesystem_writer(
    filesystem_writer: &mut FilesystemWriter<'_, '_, '_>,
    app_directory: &Path,
) -> crate::Result<()> {
    let mut directory_entries: Vec<walkdir::DirEntry> = WalkDir::new(app_directory)
        .follow_links(false)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to walk AppDirectory {}", app_directory.display()))?;

    directory_entries.sort_by_key(|entry| entry.path().components().count());

    for entry in directory_entries {
        let absolute_path = entry.path();
        if absolute_path == app_directory {
            continue;
        }

        let relative_path = absolute_path
            .strip_prefix(app_directory)
            .with_context(|| format!("Path {absolute_path:?} not under AppDirectory"))?;

        let metadata = entry
            .metadata()
            .with_context(|| format!("Failed to stat {}", absolute_path.display()))?;

        let permissions = get_cross_platform_permissions(&entry, &metadata);

        #[cfg(unix)]
        let modified_time = {
            use std::os::unix::fs::MetadataExt;
            metadata.mtime() as u32
        };
        #[cfg(not(unix))]
        let modified_time = 0u32;

        let header = NodeHeader::new(permissions, 0, 0, modified_time);

        // Entries are sorted by depth, so WalkDir has already yielded (and we
        // have already pushed) every ancestor directory by the time a child
        // arrives — no separate parent `push_dir_all` is needed.
        if entry.file_type().is_symlink() {
            let target_path = fs::read_link(absolute_path)
                .with_context(|| format!("Failed to read symlink {}", absolute_path.display()))?;
            filesystem_writer
                .push_symlink(&target_path, relative_path, header)
                .with_context(|| {
                    format!(
                        "Failed to add symlink {} to squashfs",
                        relative_path.display()
                    )
                })?;
        } else if entry.file_type().is_dir() {
            filesystem_writer
                .push_dir_all(relative_path, header)
                .with_context(|| {
                    format!(
                        "Failed to add directory {} to squashfs",
                        relative_path.display()
                    )
                })?;
        } else if entry.file_type().is_file() {
            let file = File::open(absolute_path).with_context(|| {
                format!("Failed to open {} for squashfs", absolute_path.display())
            })?;
            filesystem_writer
                .push_file(file, relative_path, header)
                .with_context(|| {
                    format!("Failed to add file {} to squashfs", relative_path.display())
                })?;
        }
    }
    Ok(())
}

fn concatenate_payloads(
    runtime_bytes: &[u8],
    squash_filesystem_path: &Path,
    package_path: &Path,
) -> crate::Result<()> {
    if let Some(parent_directory) = package_path.parent() {
        fs::create_dir_all(parent_directory)
            .with_context(|| format!("Failed to create {}", parent_directory.display()))?;
    }

    let mut output_writer = BufWriter::new(
        File::create(package_path)
            .with_context(|| format!("Failed to create {}", package_path.display()))?,
    );
    output_writer
        .write_all(runtime_bytes)
        .with_context(|| "Failed to write AppImage runtime")?;

    let mut squash_reader = BufReader::new(
        File::open(squash_filesystem_path)
            .with_context(|| format!("Failed to open {}", squash_filesystem_path.display()))?,
    );

    std::io::copy(&mut squash_reader, &mut output_writer)
        .with_context(|| "Failed to append squashfs payload")?;

    output_writer.flush()?;
    Ok(())
}

fn get_cross_platform_permissions(entry: &walkdir::DirEntry, metadata: &std::fs::Metadata) -> u16 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = entry;
        (metadata.mode() & 0o7777) as u16
    }
    #[cfg(not(unix))]
    {
        if entry.file_type().is_dir() {
            0o755
        } else {
            0o644
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn backhand_writes_minimal_squashfs() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("AppDir");
        fs::create_dir_all(app_dir.join("usr/bin")).unwrap();
        let mut f = File::create(app_dir.join("usr/bin/hello")).unwrap();
        f.write_all(b"#!/bin/sh\necho hi\n").unwrap();

        let out = dir.path().join("payload.squashfs");
        build_via_backhand(&app_dir, &out, Compressor::Gzip).unwrap();
        assert!(out.metadata().unwrap().len() > 0);
    }

    #[test]
    fn backhand_writes_nested_dir_and_symlink() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("AppDir");
        fs::create_dir_all(app_dir.join("usr/bin")).unwrap();
        fs::create_dir_all(app_dir.join("usr/share/apps")).unwrap();
        let mut f = File::create(app_dir.join("usr/bin/hello")).unwrap();
        f.write_all(b"#!/bin/sh\necho hi\n").unwrap();
        // create a symlink: AppDir/AppRun -> usr/bin/hello
        #[cfg(unix)]
        std::os::unix::fs::symlink("usr/bin/hello", app_dir.join("AppRun")).unwrap();

        let out = dir.path().join("payload.squashfs");
        build_via_backhand(&app_dir, &out, Compressor::Gzip).unwrap();
        assert!(out.metadata().unwrap().len() > 0);
    }
}
