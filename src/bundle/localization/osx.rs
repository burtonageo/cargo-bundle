//! macOS `*.lproj/InfoPlist.strings` localization.

use super::sorted_locales;
use crate::bundle::common;
use anyhow::Context;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Locale-keyed InfoPlist string tables for macOS app bundles.
///
/// Mirrors the Cargo.toml shape:
///
/// ```toml
/// [package.metadata.bundle.osx.localizations.fr]
/// CFBundleDisplayName = "Mon Application"
/// ```
#[derive(Clone, Copy, Debug)]
pub struct OsxLocalizations<'a> {
    localized_strings: &'a HashMap<String, HashMap<String, String>>,
}

impl<'a> OsxLocalizations<'a> {
    pub fn new(localized_strings: &'a HashMap<String, HashMap<String, String>>) -> Self {
        Self { localized_strings }
    }

    /// Writes `*.lproj/InfoPlist.strings` under the provided `output_root`
    /// (typically `Contents/Resources`).
    pub fn write_to_directory(&self, output_root: &Path) -> crate::Result<()> {
        for (locale_code, strings_mapping) in sorted_locales(self.localized_strings) {
            self.write_single_locale_directory(output_root, locale_code, strings_mapping)?;
        }
        Ok(())
    }

    /// Handles the directory creation and file initialization for a single locale.
    fn write_single_locale_directory(
        &self,
        output_root: &Path,
        locale_code: &str,
        strings_mapping: &HashMap<String, String>,
    ) -> crate::Result<()> {
        let localization_directory = output_root.join(locale_code).with_extension("lproj");

        fs::create_dir_all(&localization_directory).with_context(|| {
            format!("Failed to create localization directory at {localization_directory:?}")
        })?;

        let strings_file_path = localization_directory.join("InfoPlist.strings");
        let mut output_file = common::create_file(&strings_file_path)?;

        Self::write_key_value_pairs(&mut output_file, strings_mapping)?;
        output_file.flush()?;

        Ok(())
    }

    /// Formats and writes the specific key-value string pairs into the target file.
    fn write_key_value_pairs(
        output_file: &mut impl Write,
        strings_mapping: &HashMap<String, String>,
    ) -> crate::Result<()> {
        for (key, value) in strings_mapping {
            writeln!(
                output_file,
                "{key} = \"{}\";",
                escape_legacy_apple_string(value)
            )?;
        }
        Ok(())
    }
}

/// Escapes embedded backslashes and double-quotes for the legacy `.strings` file format.
fn escape_legacy_apple_string(value: &str) -> Cow<'_, str> {
    if !value.contains(['\\', '"']) {
        return Cow::Borrowed(value);
    }

    let mut escaped_string = String::with_capacity(value.len() + 2);
    for character in value.chars() {
        match character {
            '\\' => escaped_string.push_str(r"\\"),
            '"' => escaped_string.push_str(r#"\""#),
            _ => escaped_string.push(character),
        }
    }
    Cow::Owned(escaped_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_to_directory_creates_lproj_strings() {
        let mut localized_strings = HashMap::new();
        localized_strings.insert(
            "fr".to_string(),
            HashMap::from([("CFBundleDisplayName".into(), "Mon Application".into())]),
        );
        localized_strings.insert(
            "de".to_string(),
            HashMap::from([("CFBundleDisplayName".into(), "Meine Anwendung".into())]),
        );

        let localizations = OsxLocalizations::new(&localized_strings);
        let temporary_directory = tempdir().unwrap();

        localizations
            .write_to_directory(temporary_directory.path())
            .unwrap();

        let french_output = std::fs::read_to_string(
            temporary_directory
                .path()
                .join("fr.lproj/InfoPlist.strings"),
        )
        .unwrap();
        assert!(french_output.contains(r#"CFBundleDisplayName = "Mon Application";"#));

        let german_output = std::fs::read_to_string(
            temporary_directory
                .path()
                .join("de.lproj/InfoPlist.strings"),
        )
        .unwrap();
        assert!(german_output.contains(r#"CFBundleDisplayName = "Meine Anwendung";"#));
    }

    #[test]
    fn write_to_directory_escapes_quotes() {
        let mut localized_strings = HashMap::new();
        localized_strings.insert(
            "en".to_string(),
            HashMap::from([("CFBundleName".into(), r#"Say "hi""#.into())]),
        );

        let localizations = OsxLocalizations::new(&localized_strings);
        let temporary_directory = tempdir().unwrap();

        localizations
            .write_to_directory(temporary_directory.path())
            .unwrap();

        let english_output = std::fs::read_to_string(
            temporary_directory
                .path()
                .join("en.lproj/InfoPlist.strings"),
        )
        .unwrap();
        assert!(english_output.contains(r#"CFBundleName = "Say \"hi\"";"#));
    }
}
