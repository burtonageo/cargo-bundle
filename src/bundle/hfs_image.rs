//! Writes a compressed HFS+ disk image from a directory, in pure Rust.

use anyhow::Context;
use fstool::{
    block::{
        BlockDevice, FileBackend,
        dmg::{UdifWriteOptions, write_udif},
    },
    fs::{
        FsSizePlan,
        hfs_plus::{
            FormatOpts, HfsPlus,
            size_plan::{GeneratedBudget, HfsPlusSizePlan},
        },
    },
    repack::{FsSink, Source, walk_source_into_sink},
};
use std::path::Path;

/// Space set aside for files that `finish` creates after the tree is copied:
/// the `Applications` link and `.DS_Store`, which is a few kilobytes.
const GENERATED_FILES: u64 = 2;
const GENERATED_BYTES: u64 = 1024 * 1024;

/// Copies `tree` into a new HFS+ volume and writes it to `output` as a UDIF
/// (`.dmg`) image.
///
/// `finish` runs once the tree is in place. The Finder files it creates must
/// refer to catalog IDs that only exist after that point.
pub(super) fn write_image(
    tree: &Path,
    output: &Path,
    volume_name: &str,
    finish: impl FnOnce(&mut HfsPlus, &mut dyn BlockDevice) -> crate::Result<()>,
) -> crate::Result<()> {
    let mut options = FormatOpts::default();
    options.volume_name = volume_name.into();

    let source = Source::HostDir(tree.into());
    let mut plan = HfsPlusSizePlan::new(&options);
    fstool::analyze::plan_size(&source, &mut plan)?;
    let mut generated = GeneratedBudget::default();
    generated.files = GENERATED_FILES;
    generated.data_bytes = GENERATED_BYTES;
    plan.add_budget(&generated);
    plan.apply_to(&mut options);

    let raw = tempfile::NamedTempFile::new().context("Failed to create the raw volume")?;
    let mut device = FileBackend::create(raw.path(), plan.total_size())?;
    let mut volume = HfsPlus::format(&mut device, &options)?;
    walk_source_into_sink(&source, &mut FsSink::new(&mut volume, &mut device))?;
    finish(&mut volume, &mut device)?;
    volume.flush(&mut device)?;
    device.sync()?;

    let output_file =
        std::fs::File::create(output).with_context(|| format!("Failed to create {output:?}"))?;
    write_udif(&mut device, output_file, &UdifWriteOptions::default())?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use fstool::{block::dmg::DmgBackend, fs::Filesystem};
    use std::{
        fs,
        io::Read,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn copies_modes_links_and_a_large_catalog_then_runs_finish() {
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path().join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("payload"), b"payload bytes").unwrap();
        fs::set_permissions(tree.join("payload"), fs::Permissions::from_mode(0o751)).unwrap();
        symlink("payload", tree.join("alias")).unwrap();
        // Enough entries to force the catalog B-tree past a single node.
        for n in 0..800 {
            fs::write(tree.join(format!("entry-{n:04}")), []).unwrap();
        }
        let image = temp.path().join("test.dmg");
        write_image(&tree, &image, "Test Volume", |volume, device| {
            let body = b"generated";
            volume.create_file(device, "/generated", &mut &body[..], 9, 0o644, 0, 0, 0)?;
            Ok(())
        })
        .unwrap();

        let mut device = DmgBackend::open(&image).unwrap();
        let report =
            fstool::fs::hfs_plus::verify::verify(&mut device, &Default::default()).unwrap();
        assert!(report.is_clean(), "{:?}", report.diagnostics);
        let mut volume = HfsPlus::open(&mut device).unwrap();
        assert_eq!(volume.volume_name(), "Test Volume");
        let mode = volume
            .getattr(&mut device, Path::new("/payload"))
            .unwrap()
            .mode;
        assert_eq!(mode & 0o777, 0o751);
        assert_eq!(
            volume
                .read_symlink_target_path(&mut device, "/alias")
                .unwrap(),
            "payload"
        );
        let mut generated = String::new();
        volume
            .open_file_reader(&mut device, "/generated")
            .unwrap()
            .read_to_string(&mut generated)
            .unwrap();
        assert_eq!(generated, "generated");
        assert_eq!(volume.list_path(&mut device, "/").unwrap().len(), 800 + 3);
    }

    #[test]
    fn a_failing_finish_step_fails_the_image() {
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path().join("tree");
        fs::create_dir(&tree).unwrap();
        let error = write_image(&tree, &temp.path().join("x.dmg"), "Test", |_, _| {
            anyhow::bail!("finish failed")
        })
        .unwrap_err();
        assert!(error.to_string().contains("finish failed"));
    }
}
