//! Keeps the guide in step with the code: every setting and format must be
//! mentioned, and every page and link must exist.

use std::{collections::BTreeSet, fs, path::Path};

fn read(path: impl AsRef<Path>) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    // A Windows checkout may have CRLF line endings.
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path:?}: {error}"))
        .replace("\r\n", "\n")
}

/// All pages of the guide, as (path relative to docs/src, text).
fn pages() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/src");
    let mut pages = Vec::new();
    let mut directories = vec![root.clone()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                let name = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                pages.push((
                    name,
                    fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
                ));
            }
        }
    }
    pages
}

/// Field names of `struct <name> { ... }` in the settings source.
fn fields(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("struct {name} {{"))
        .unwrap_or_else(|| panic!("struct {name} not found"));
    source[start..]
        .lines()
        .skip(1)
        .take_while(|line| *line != "}")
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix("pub ").unwrap_or(line);
            let (field, _) = line.split_once(": ")?;
            field
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_')
                .then(|| field.to_owned())
        })
        .collect()
}

#[test]
fn every_setting_is_documented() {
    let source = read("src/bundle/settings.rs");
    let guide: String = pages().into_iter().map(|(_, text)| text).collect();
    let mut missing = BTreeSet::new();
    for table in [
        "BundleSettings",
        "OsxSettings",
        "IosSettings",
        "AndroidSettings",
        "LinuxSettings",
        "WindowsSigningSettings",
        "LinuxSigningSettings",
    ] {
        let found = fields(&source, table);
        assert!(!found.is_empty(), "no fields parsed for {table}");
        for field in found {
            // As a table cell, as an assignment, or as a table header.
            let mentioned = [
                format!("`{field}`"),
                format!("{field} = "),
                format!("bundle.{field}"),
            ]
            .iter()
            .any(|form| guide.contains(form));
            if !mentioned {
                missing.insert(format!("{table}.{field}"));
            }
        }
    }
    assert!(missing.is_empty(), "undocumented settings: {missing:?}");
}

#[test]
fn every_format_is_documented() {
    let source = read("src/bundle/settings.rs");
    let formats = read("docs/src/formats.md");
    let names: Vec<_> = source
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("#[value(name = \"")?
                .strip_suffix("\")]")
        })
        .collect();
    assert!(names.len() >= 11, "{names:?}");
    for name in names {
        assert!(
            formats.contains(&format!("| `{name}` |")),
            "format {name} is not in formats.md"
        );
    }
}

#[test]
fn pages_are_listed_and_links_resolve() {
    let summary = read("docs/src/SUMMARY.md");
    let pages = pages();
    for (name, text) in &pages {
        if name != "SUMMARY.md" {
            assert!(
                summary.contains(&format!("({name})")),
                "{name} is not in SUMMARY.md"
            );
        }
        let directory = Path::new(name).parent().unwrap();
        for target in text
            .split("](")
            .skip(1)
            .filter_map(|rest| rest.split(')').next())
        {
            let file = target.split('#').next().unwrap();
            if file.is_empty() || file.contains("://") {
                continue;
            }
            let resolved = directory.join(file);
            let mut parts = Vec::new();
            for part in resolved.components() {
                match part.as_os_str().to_str().unwrap() {
                    ".." => drop(parts.pop()),
                    part => parts.push(part.to_owned()),
                }
            }
            let resolved = parts.join("/");
            assert!(
                pages.iter().any(|(page, _)| *page == resolved),
                "{name} links to missing page {target}"
            );
        }
    }
}

/// The ```toml blocks of every page, with the page they are on.
fn toml_blocks() -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    for (page, text) in pages() {
        for rest in text.split("```toml\n").skip(1) {
            blocks.push((page.clone(), rest.split("```").next().unwrap().to_owned()));
        }
    }
    blocks
}

/// Run `cargo bundle --format deb` on a package whose manifest is `manifest`,
/// with a stand-in for the executable, and return the finished process.
fn bundle(manifest: &str) -> (tempfile::TempDir, std::process::Output) {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(temp.path().join("Cargo.toml"), manifest).unwrap();
    fs::write(temp.path().join("prebuilt"), "binary").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cargo-bundle"))
        .args(["bundle", "--format", "deb", "--binary-path", "prebuilt"])
        .current_dir(temp.path())
        .env("TERM", "dumb")
        .output()
        .unwrap();
    (temp, output)
}

#[test]
fn the_first_bundle_in_the_introduction_works() {
    let intro = read("docs/src/index.md");
    let manifest = intro
        .split("```toml\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap();
    let (temp, output) = bundle(manifest);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(".deb"), "{stdout}");
    assert!(temp.path().join("target/debug/bundle/deb").is_dir());
}

#[test]
fn every_manifest_example_in_the_guide_is_accepted() {
    let header = "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
    let mut checked = 0;
    for (page, block) in toml_blocks() {
        if !block.contains("[package.metadata.bundle") && !block.contains("[package]") {
            continue;
        }
        let manifest = if block.contains("[package]") {
            block.clone()
        } else if block.contains("[package.metadata.bundle]") {
            format!("{header}{block}")
        } else {
            format!(
                "{header}[package.metadata.bundle]\nidentifier = \"com.example.my-app\"\n{block}"
            )
        };
        let (_, output) = bundle(&manifest);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Examples may name files that do not exist here; settings must still
        // parse, validate and agree with the code.
        for rejected in [
            "unknown field",
            "invalid type",
            "invalid value",
            "missing field",
            "duplicate",
            "Invalid locale",
            "Use only one",
            "must be a table",
            "not a platform override",
            "TOML parse error",
        ] {
            assert!(
                !stderr.contains(rejected),
                "{page}: {rejected}\n{block}\n{stderr}"
            );
        }
        checked += 1;
    }
    assert!(checked >= 10, "only {checked} examples were checked");
}
