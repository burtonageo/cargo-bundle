//! Shared localization API for platform bundles.
//!
//! Cargo.toml stores locale tables as:
//!
//! ```toml
//! [package.metadata.bundle.osx_localizations.fr]
//! CFBundleDisplayName = "Mon Application"
//!
//! [package.metadata.bundle.linux_localizations.fr]
//! Name = "Mon App"
//! Comment = "Une description"
//! ```
//!
//! Both shapes are a locale-code → platform-specific strings map. Platform
//! writers wrap that map and expose methods directly as inherent implementations.

mod linux;
mod osx;

pub use linux::{LinuxDesktopLocale, LinuxDesktopLocalizations};
pub use osx::OsxLocalizations;

// Re-exported for unit tests and any future desktop-entry helpers.
#[cfg(test)]
pub use linux::DesktopKeywords;

// Used by the FreeDesktop desktop-file renderer.
pub(crate) use linux::escape_desktop_value;

use std::collections::{BTreeMap, HashMap};

/// Locales sorted by code for deterministic artifact output.
pub(crate) fn sorted_locales<E>(map: &HashMap<String, E>) -> BTreeMap<&str, &E> {
    map.iter()
        .map(|(locale, entry)| (strip_locale_encoding(locale), entry))
        .collect()
}

/// Strip a `.ENCODING` component from a locale code, preserving any
/// `@MODIFIER` suffix.
fn strip_locale_encoding(locale: &str) -> &str {
    match locale.split_once('.') {
        Some((prefix, rest)) if !rest.contains('@') => prefix,
        _ => locale,
    }
}

/// Preferred fallback locales when a platform needs an unlocalized base value.
pub(crate) const FALLBACK_LOCALE_PREFERENCE: &[&str] = &["C", "en"];
