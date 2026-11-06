//! Builds a DMG through the CLI and reads the image back with an independent
//! HFS+ reader, checking the Finder layout against the volume's own catalog.

#![cfg(unix)]

use dmg_layout::AliasRecord;
use ds_parser::{FieldKey, Value as DsValue};
use fstool::{
    block::{
        BlockDevice,
        dmg::{DmgBackend, verify_udif},
    },
    fs::{
        Filesystem,
        hfs_plus::{HfsPlus, verify::verify},
    },
};
use std::{fs, io::Read, os::unix::fs::PermissionsExt, path::Path};

mod common;
use common::{project, succeeded};

const BACKGROUND_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="500" height="300">
  <rect width="500" height="300" fill="#e8ecf4"/>
  <rect id="app" x="80" y="120" width="96" height="96" fill="none"/>
  <rect id="applications" x="320" y="120" width="96" height="96" fill="none"/>
</svg>
"##;

fn read(volume: &HfsPlus, device: &mut dyn BlockDevice, path: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    volume
        .open_file_reader(device, path)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn dmg_holds_the_app_and_a_finder_layout_that_matches_its_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(
        temp.path(),
        "\n[package.metadata.bundle.osx]\ndmg_background = \"background.svg\"\n",
    );
    let background = temp.path().join("background.svg");
    fs::write(&background, BACKGROUND_SVG).unwrap();
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();

    succeeded(&bundle(&["--format", "dmg"], &binary));
    let image = temp.path().join("target/debug/bundle/dmg/Post Card.dmg");
    assert!(verify_udif(&image).unwrap().is_valid());
    let mut device = DmgBackend::open(&image).unwrap();
    let report = verify(&mut device, &Default::default()).unwrap();
    assert!(report.is_clean(), "{:?}", report.diagnostics);

    let mut volume = HfsPlus::open(&mut device).unwrap();
    let executable = "/Post Card.app/Contents/MacOS/postcard";
    let mode = volume
        .getattr(&mut device, Path::new(executable))
        .unwrap()
        .mode;
    assert_eq!(mode & 0o777, 0o755);
    assert_eq!(
        read(&volume, &mut device, executable),
        fs::read(&binary).unwrap()
    );
    assert_eq!(
        volume
            .read_symlink_target_path(&mut device, "/Applications")
            .unwrap(),
        "/Applications"
    );
    assert!(!read(&volume, &mut device, "/.background/background.tiff").is_empty());

    // Finder follows an alias to the background by catalog ID, so the IDs in
    // the .DS_Store must be the ones this volume assigned.
    let ds_store = ds_parser::parse(&read(&volume, &mut device, "/.DS_Store")).unwrap();
    assert!(ds_store.warnings.is_empty(), "{:?}", ds_store.warnings);
    let icon_view = ds_store
        .records
        .iter()
        .find(|record| record.name == "." && record.field == FieldKey::icvp)
        .expect("no icon view record");
    let DsValue::IconViewProperties(properties) = &icon_view.value else {
        panic!("unexpected icvp value: {:?}", icon_view.value);
    };
    let alias = properties
        .unknown
        .get("backgroundImageAlias")
        .and_then(plist::Value::as_data)
        .expect("no background alias");
    let alias = AliasRecord::from_bytes(alias).unwrap();
    let directory = volume.entry_info("/.background").unwrap();
    let file = volume.entry_info("/.background/background.tiff").unwrap();
    assert_eq!(alias.header.parent_catalog_node_id.0, directory.cnid);
    assert_eq!(alias.header.target_catalog_node_id.0, file.cnid);
    assert_eq!(alias.header.target_date.0, file.create_date);
    assert_eq!(alias.header.volume_name, b"Post Card");
    for name in ["Applications", "Post Card.app"] {
        assert!(
            ds_store
                .records
                .iter()
                .any(|r| r.name == name && r.field == FieldKey::Iloc),
            "no icon position for {name}"
        );
    }
    drop(device);

    // A failed rebuild keeps the previous image.
    let original = fs::read(&image).unwrap();
    fs::write(&background, "<svg broken").unwrap();
    let failed = bundle(&["--format", "dmg"], &binary);
    assert!(!failed.status.success());
    assert_eq!(fs::read(&image).unwrap(), original);
}

#[test]
fn dmg_without_a_background_is_just_the_app_and_the_link() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(temp.path(), "");
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "binary").unwrap();
    succeeded(&bundle(&["--format", "dmg"], &binary));

    let image = temp.path().join("target/debug/bundle/dmg/Post Card.dmg");
    let mut device = DmgBackend::open(&image).unwrap();
    let volume = HfsPlus::open(&mut device).unwrap();
    let mut names: Vec<_> = volume
        .list_path(&mut device, "/")
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    assert_eq!(names, ["Applications", "Post Card.app"]);
}
