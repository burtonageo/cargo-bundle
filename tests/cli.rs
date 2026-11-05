//! Checks that apply to every format.

use std::fs;

mod common;
use common::{project, succeeded};

#[test]
fn an_unusable_bundle_name_fails_before_anything_is_written() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(temp.path(), "");
    let manifest = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        manifest.replace("Post Card", "../../escape"),
    )
    .unwrap();
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "binary").unwrap();

    for format in ["osx", "dmg", "ios", "deb", "appimage"] {
        let output = bundle(&["--format", format], &binary);
        assert!(!output.status.success(), "{format}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("must be a single file name"), "{stderr}");
    }
    assert!(!temp.path().join("target").exists());
    assert!(!temp.path().parent().unwrap().join("escape").exists());
}

#[test]
fn localizations_reach_the_desktop_file_and_lproj_directories() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(
        temp.path(),
        r#"
[package.metadata.bundle.linux.localizations."fr.UTF-8"]
Name = "Carte postale"
[package.metadata.bundle.osx.localizations."fr.UTF-8"]
CFBundleDisplayName = "Carte \"postale\""
"#,
    );
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "binary").unwrap();

    succeeded(&bundle(
        &["--format", "deb", "--target", "aarch64-unknown-linux-gnu"],
        &binary,
    ));
    let desktop = temp.path().join(
        "target/aarch64-unknown-linux-gnu/debug/bundle/deb/postcard_1.2.3_arm64/data/usr/share/applications/postcard.desktop",
    );
    assert!(
        fs::read_to_string(desktop)
            .unwrap()
            .contains("Name[fr]=Carte postale\n")
    );

    succeeded(&bundle(
        &["--format", "osx", "--target", "aarch64-apple-darwin"],
        &binary,
    ));
    let strings = temp.path().join(
        "target/aarch64-apple-darwin/debug/bundle/osx/Post Card.app/Contents/Resources/fr.lproj/InfoPlist.strings",
    );
    assert_eq!(
        fs::read_to_string(strings).unwrap(),
        "\"CFBundleDisplayName\" = \"Carte \\\"postale\\\"\";\n"
    );
}

#[test]
fn unsafe_or_colliding_locale_codes_are_rejected_when_settings_load() {
    let binary_in = |dir: &std::path::Path| {
        let binary = dir.join("prebuilt");
        fs::write(&binary, "binary").unwrap();
        binary
    };
    for (table, message) in [
        (
            "osx.localizations.\"../escape\"",
            "Invalid osx.localizations",
        ),
        (
            "linux.localizations.\"fr/x\"",
            "Invalid linux.localizations",
        ),
        ("linux.localizations.en", "both mean \"en\""),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let extra = if message.contains("both") {
            format!(
                "[package.metadata.bundle.linux.localizations.\"en.UTF-8\"]\nName = \"x\"\n[package.metadata.bundle.{table}]\nName = \"y\"\n"
            )
        } else {
            format!("[package.metadata.bundle.{table}]\nName = \"x\"\n")
        };
        let bundle = project(temp.path(), &format!("\n{extra}"));
        let output = bundle(&["--format", "deb"], &binary_in(temp.path()));
        assert!(!output.status.success(), "{table}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{table}: {stderr}");
    }
}

#[test]
fn notarization_that_cannot_succeed_is_refused_before_anything_is_written() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(
        temp.path(),
        "apple_notarization_api_key = \"notary.json\"\n",
    );
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "binary").unwrap();

    for format in ["osx", "dmg"] {
        let output = bundle(&["--format", format], &binary);
        assert!(!output.status.success(), "{format}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Apple notarization requires"), "{stderr}");
    }
    assert!(!temp.path().join("target").exists());

    // The setting only concerns macOS output; other formats ignore it.
    succeeded(&bundle(&["--format", "deb"], &binary));
}

#[test]
fn rpm_is_reported_as_unimplemented_instead_of_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(temp.path(), "");
    let binary = temp.path().join("prebuilt");
    fs::write(&binary, "binary").unwrap();
    let output = bundle(&["--format", "rpm"], &binary);
    assert_eq!(output.status.code(), Some(1), "an error, not a panic (101)");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not implemented"));
}
