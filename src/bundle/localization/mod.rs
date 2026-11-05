//! Shared localization API for platform bundles.
//!
//! Cargo.toml stores locale tables as:
//!
//! ```toml
//! [package.metadata.bundle.osx.localizations.fr]
//! CFBundleDisplayName = "Mon Application"
//!
//! [package.metadata.bundle.linux.localizations.fr]
//! Name = "Mon App"
//! Comment = "Une description"
//! ```
//!
//! Both shapes are a locale-code → platform-specific strings map. Platform
//! writers wrap that map and expose methods directly as inherent implementations.

mod apple;
mod linux;

pub use apple::AppleLocalizations;
pub use linux::{LinuxDesktopLocale, LinuxDesktopLocalizations};

// Re-exported for unit tests and any future desktop-entry helpers.
#[cfg(test)]
pub use linux::DesktopKeywords;

// Used by the FreeDesktop desktop-file renderer.
pub(crate) use linux::escape_desktop_value;

use std::collections::{BTreeMap, HashMap};

/// Drops the `.ENCODING` part of a POSIX locale code, keeping any `@modifier`:
/// `de_DE.UTF-8@euro` becomes `de_DE@euro`.
fn normalized_locale_code(locale: &str) -> String {
    let (base, modifier) = match locale.split_once('@') {
        Some((base, modifier)) => (base, Some(modifier)),
        None => (locale, None),
    };
    let language = base.split_once('.').map_or(base, |(language, _)| language);
    match modifier {
        Some(modifier) => format!("{language}@{modifier}"),
        None => language.to_owned(),
    }
}

/// Locales by normalized code, in order, for deterministic artifact output.
/// Call [`validate_locale_codes`] first: colliding codes would overwrite each other.
pub(crate) fn sorted_locales<E>(map: &HashMap<String, E>) -> BTreeMap<String, &E> {
    map.iter()
        .map(|(locale, entry)| (normalized_locale_code(locale), entry))
        .collect()
}

/// Rejects locale codes that cannot name a directory or desktop-file key, and
/// codes such as `en` and `en.UTF-8` that normalize to the same locale.
pub(crate) fn validate_locale_codes<E>(map: &HashMap<String, E>) -> crate::Result<()> {
    let mut seen = HashMap::new();
    let mut codes: Vec<_> = map.keys().collect();
    codes.sort();
    for locale in codes {
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '@');
        anyhow::ensure!(
            locale.starts_with(|c: char| c.is_ascii_alphanumeric()) && locale.chars().all(allowed),
            "Invalid locale code {locale:?}: use letters, digits, `_`, `-`, and an optional `.encoding` or `@modifier`"
        );
        let normalized = normalized_locale_code(locale);
        if let Some(other) = seen.insert(normalized.clone(), locale) {
            anyhow::bail!("Locale codes {other:?} and {locale:?} both mean {normalized:?}");
        }
    }
    Ok(())
}

/// Preferred fallback locales when a platform needs an unlocalized base value.
pub(crate) const FALLBACK_LOCALE_PREFERENCE: &[&str] = &["C", "en"];

#[cfg(test)]
mod tests {
    use super::*;

    fn locales(codes: &[&str]) -> HashMap<String, ()> {
        codes.iter().map(|code| (code.to_string(), ())).collect()
    }

    #[test]
    fn encodings_are_dropped_and_modifiers_kept() {
        let map = locales(&["de_DE.UTF-8@euro", "fr", "sr@latin"]);
        let codes: Vec<_> = sorted_locales(&map).into_keys().collect();
        assert_eq!(codes, ["de_DE@euro", "fr", "sr@latin"]);
    }

    #[test]
    fn unsafe_and_colliding_codes_are_rejected() {
        assert!(validate_locale_codes(&locales(&["en", "fr_CA", "zh-Hans", "sr@latin"])).is_ok());
        for bad in [
            "", ".", "..", "../x", "a/b", "a\\b", "en US", "@euro", ".UTF-8",
        ] {
            assert!(validate_locale_codes(&locales(&[bad])).is_err(), "{bad:?}");
        }
        let error = validate_locale_codes(&locales(&["en", "en.UTF-8"])).unwrap_err();
        assert!(error.to_string().contains("both mean \"en\""), "{error}");
    }
}
