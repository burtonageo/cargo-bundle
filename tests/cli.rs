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
        ("osx.localizations.\"../escape\"", "Invalid locale code"),
        ("linux.localizations.\"fr/x\"", "Invalid locale code"),
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

fn prebuilt(dir: &std::path::Path) -> std::path::PathBuf {
    let binary = dir.join("prebuilt");
    fs::write(&binary, "binary").unwrap();
    binary
}

fn desktop_file(dir: &std::path::Path) -> String {
    fs::read_to_string(dir.join(
        "target/aarch64-unknown-linux-gnu/debug/bundle/deb/postcard_1.2.3_arm64/data/usr/share/applications/postcard.desktop",
    ))
    .unwrap()
}

#[test]
fn a_platform_table_shadows_shared_settings_for_its_target_only() {
    for apple_table in ["osx", "macos"] {
        let temp = tempfile::tempdir().unwrap();
        let bundle = project(
            temp.path(),
            &format!(
                "[package.metadata.bundle.linux]\nname = \"Linux Card\"\n\
                 [package.metadata.bundle.{apple_table}]\nname = \"Mac Card\"\n\
                 [package.metadata.bundle.windows]\nname = \"Windows Card\"\n"
            ),
        );
        let binary = prebuilt(temp.path());

        succeeded(&bundle(
            &["--format", "deb", "--target", "aarch64-unknown-linux-gnu"],
            &binary,
        ));
        assert!(desktop_file(temp.path()).contains("Name=Linux Card\n"));

        succeeded(&bundle(
            &["--format", "osx", "--target", "aarch64-apple-darwin"],
            &binary,
        ));
        assert!(
            temp.path()
                .join("target/aarch64-apple-darwin/debug/bundle/osx/Mac Card.app")
                .is_dir(),
            "{apple_table}"
        );
    }
}

#[test]
fn platform_tables_that_are_wrong_fail_even_when_another_platform_is_built() {
    for (tables, message) in [
        (
            "[package.metadata.bundle.windows]\nnot_a_setting = 1\n",
            "not_a_setting",
        ),
        (
            "[package.metadata.bundle.osx]\nname = \"a\"\n[package.metadata.bundle.macos]\nname = \"b\"\n",
            "Use only one of",
        ),
        (
            "[package.metadata.bundle.ios]\nlinux = {}\n",
            "not a platform override",
        ),
        ("apple_notarization_timeout = 0\n", "nonzero"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let bundle = project(temp.path(), tables);
        let output = bundle(&["--format", "deb"], &prebuilt(temp.path()));
        assert!(!output.status.success(), "{tables}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{tables}: {stderr}");
    }
}

#[test]
fn localization_files_merge_in_order_and_inline_values_win() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(
        temp.path(),
        r#"
localization_files = ["locales/a.toml", "locales/b.toml"]
[package.metadata.bundle.linux.localizations.fr]
Name = "Inline"
"#,
    );
    fs::create_dir(temp.path().join("locales")).unwrap();
    fs::write(
        temp.path().join("locales/a.toml"),
        "[linux.fr]\nName = \"A\"\nComment = \"from a\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("locales/b.toml"),
        "[linux.fr]\nComment = \"from b\"\n[linux.de]\nName = \"Deutsch\"\n",
    )
    .unwrap();
    succeeded(&bundle(
        &["--format", "deb", "--target", "aarch64-unknown-linux-gnu"],
        &prebuilt(temp.path()),
    ));
    let desktop = desktop_file(temp.path());
    for line in [
        "Name[fr]=Inline\n",
        "Comment[fr]=from b\n",
        "Name[de]=Deutsch\n",
    ] {
        assert!(desktop.contains(line), "{line}: {desktop}");
    }
}

#[test]
fn a_bad_localization_file_fails_whichever_platform_it_targets() {
    for (contents, message) in [
        (
            "[macos.\"../escape\"]\nA = \"b\"\n",
            "Invalid localization file",
        ),
        ("[linux.fr]\nNmae = \"typo\"\n", "Nmae"),
        ("[windows.fr]\nA = \"b\"\n", "windows"),
        ("not toml [", "TOML parse error"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let bundle = project(temp.path(), "localization_files = [\"fr.toml\"]\n");
        fs::write(temp.path().join("fr.toml"), contents).unwrap();
        let output = bundle(&["--format", "deb"], &prebuilt(temp.path()));
        assert!(!output.status.success(), "{contents}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{contents}: {stderr}");
    }
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(temp.path(), "localization_files = [\"missing.toml\"]\n");
    let output = bundle(&["--format", "deb"], &prebuilt(temp.path()));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to read localization file"));
}

#[test]
fn target_combinations_that_cannot_work_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(
        temp.path(),
        "apple_notarization_api_key = \"notary.json\"\n",
    );
    let binary = prebuilt(temp.path());
    for (args, message) in [
        (
            &[
                "--target",
                "aarch64-apple-darwin",
                "--target",
                "x86_64-unknown-linux-gnu",
            ][..],
            "same operating system",
        ),
        (
            &[
                "--target",
                "x86_64-unknown-linux-gnu",
                "--target",
                "aarch64-unknown-linux-gnu",
            ][..],
            "only for universal macOS bundles",
        ),
        (&["--format", "apk"][..], "requires an Android target"),
        (
            &["--format", "ios", "--target", "aarch64-apple-ios"][..],
            "not iOS bundles or IPAs",
        ),
    ] {
        let output = bundle(args, &binary);
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

#[test]
fn values_that_cannot_be_used_are_rejected_where_they_are_read() {
    for (settings, message) in [
        ("appimage_compression = \"zip\"\n", "unknown variant `zip`"),
        (
            "[package.metadata.bundle.linux.desktop_actions.\"a b\"]\nName = \"x\"\n",
            "Invalid desktop action identifier",
        ),
        (
            "[package.metadata.bundle.linux.desktop_actions.ok]\nName = \"x\"\n\
             [package.metadata.bundle.linux.desktop_actions.ok.NameLocalized]\n\"../x\" = \"y\"\n",
            "Invalid locale code",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let bundle = project(temp.path(), settings);
        let output = bundle(&["--format", "deb"], &prebuilt(temp.path()));
        assert!(!output.status.success(), "{settings}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{settings}: {stderr}");
    }
}

#[test]
fn a_format_picks_its_platforms_settings_without_a_target() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = project(
        temp.path(),
        "[package.metadata.bundle.osx]\nname = \"Mac Card\"\n",
    );
    // No --target: the dmg is macOS output whatever the host is.
    succeeded(&bundle(&["--format", "dmg"], &prebuilt(temp.path())));
    assert!(
        temp.path()
            .join("target/debug/bundle/dmg/Mac Card.dmg")
            .is_file()
    );
}
