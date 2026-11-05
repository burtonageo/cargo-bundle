//! `*.lproj/InfoPlist.strings` localization for macOS and iOS bundles.

use super::Locales;
use anyhow::Context;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::Path;

/// Locale-keyed InfoPlist string tables for Apple app bundles.
///
/// Mirrors the Cargo.toml shape:
///
/// ```toml
/// [package.metadata.bundle.osx.localizations.fr]
/// CFBundleDisplayName = "Mon Application"
/// ```
#[derive(Clone, Copy, Debug)]
pub struct AppleLocalizations<'a> {
    localized_strings: &'a Locales<BTreeMap<String, String>>,
}

impl<'a> AppleLocalizations<'a> {
    pub fn new(localized_strings: &'a Locales<BTreeMap<String, String>>) -> Self {
        Self { localized_strings }
    }

    /// Writes `<locale>.lproj/InfoPlist.strings` under `output_root`: the
    /// bundle's `Contents/Resources` on macOS, the `.app` itself on iOS.
    /// Locales and entries are sorted so identical settings give identical files.
    pub fn write_to_directory(&self, output_root: &Path) -> crate::Result<()> {
        for (locale, strings) in self.localized_strings.iter() {
            let directory = output_root.join(format!("{locale}.lproj"));
            fs::create_dir_all(&directory).with_context(|| {
                format!("Failed to create localization directory {directory:?}")
            })?;

            let mut contents = String::new();
            for (key, value) in strings {
                let (key, value) = (escape_strings_literal(key), escape_strings_literal(value));
                writeln!(contents, "\"{key}\" = \"{value}\";")?;
            }
            let file = directory.join("InfoPlist.strings");
            fs::write(&file, contents).with_context(|| format!("Failed to write {file:?}"))?;
        }
        Ok(())
    }
}

/// Escapes a string for use between the quotes of a `.strings` entry.
fn escape_strings_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                write!(escaped, "\\U{:04X}", control as u32).expect("writing to a String");
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn writes_sorted_quoted_and_escaped_entries_per_locale() {
        let mut localized = Locales::default();
        localized.insert(
            "fr.UTF-8",
            strings(&[
                ("z key", "Say \"hi\"\n\ttab\\ \u{1}"),
                ("CFBundleDisplayName", "Mon Application"),
            ]),
        );
        localized.insert("de", strings(&[("CFBundleDisplayName", "Meine Anwendung")]));
        let temp = tempfile::tempdir().unwrap();
        AppleLocalizations::new(&localized)
            .write_to_directory(temp.path())
            .unwrap();

        let read = |locale: &str| {
            fs::read_to_string(
                temp.path()
                    .join(format!("{locale}.lproj/InfoPlist.strings")),
            )
            .unwrap()
        };
        assert_eq!(
            read("de"),
            "\"CFBundleDisplayName\" = \"Meine Anwendung\";\n"
        );
        assert_eq!(
            read("fr"),
            "\"CFBundleDisplayName\" = \"Mon Application\";\n\
             \"z key\" = \"Say \\\"hi\\\"\\n\\ttab\\\\ \\U0001\";\n"
        );
    }
}
