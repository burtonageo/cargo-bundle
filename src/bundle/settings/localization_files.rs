//! External locale files: `localization_files = ["locales/fr.toml"]` keeps
//! translations out of `Cargo.toml`. Each file holds `[linux.<locale>]`,
//! `[macos.<locale>]` (or `osx`) and `[ios.<locale>]` tables shaped like the
//! inline `localizations` ones.

use super::{BundleSettings, TargetOs, platform};
use crate::bundle::localization::{LinuxDesktopLocale, Locales};
use anyhow::Context;
use std::collections::BTreeMap;
use std::path::Path;

type Strings = BTreeMap<String, String>;

/// Locale codes are checked while the file is read, for every platform in it,
/// whether or not that platform is being built.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct LocaleFile {
    linux: Locales<LinuxDesktopLocale>,
    #[serde(alias = "osx")]
    macos: Locales<Strings>,
    ios: Locales<Strings>,
}

impl LocaleFile {
    /// Later values replace earlier ones; other keys of a locale are kept.
    fn overlay(&mut self, later: Self) {
        self.linux.overlay(later.linux, LinuxDesktopLocale::overlay);
        self.macos.overlay(later.macos, |a, b| a.extend(b));
        self.ios.overlay(later.ios, |a, b| a.extend(b));
    }
}

/// Merge the files named by `bundle.localization_files`, relative to the
/// package manifest, into the selected platform's `localizations`. Files are
/// read in order and inline values from `Cargo.toml` win over all of them.
pub(super) fn load(
    bundle: &mut BundleSettings,
    package_dir: &Path,
    target_os: TargetOs,
) -> crate::Result<()> {
    let Some(files) = bundle.localization_files.take() else {
        return Ok(());
    };
    let mut merged = LocaleFile::default();
    for file in files {
        let path = package_dir.join(file);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read localization file {path:?}"))?;
        let parsed = toml::de::Deserializer::parse(&text)
            .map_err(anyhow::Error::from)
            .and_then(platform::strict::<LocaleFile, _>)
            .with_context(|| format!("Invalid localization file {path:?}"))?;
        merged.overlay(parsed);
    }
    match target_os {
        TargetOs::Linux => {
            let linux = bundle.linux.get_or_insert_with(Default::default);
            let inline = linux.localizations.take().unwrap_or_default();
            merged.linux.overlay(inline, LinuxDesktopLocale::overlay);
            linux.localizations = Some(merged.linux);
        }
        TargetOs::MacOs => {
            let osx = bundle.osx.get_or_insert_with(Default::default);
            let inline = osx.localizations.take().unwrap_or_default();
            merged.macos.overlay(inline, |a, b| a.extend(b));
            osx.localizations = Some(merged.macos);
        }
        TargetOs::Ios => {
            let ios = bundle.ios.get_or_insert_with(Default::default);
            let inline = ios.localizations.take().unwrap_or_default();
            merged.ios.overlay(inline, |a, b| a.extend(b));
            ios.localizations = Some(merged.ios);
        }
        TargetOs::Windows | TargetOs::Android => {}
    }
    Ok(())
}
