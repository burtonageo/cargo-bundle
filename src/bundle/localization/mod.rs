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

use serde::{Deserialize, Deserializer, de::Error};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// A POSIX locale code, `language[.encoding][@modifier]`, as `fr`, `pt_BR`,
/// `zh-Hans` or `sr@latin`. The encoding is dropped (`fr.UTF-8` is `fr`) and
/// the modifier kept. Each part is a non-empty run of letters, digits, `_` and
/// `-`, so a locale can always safely name a directory or a key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Locale(String);

impl FromStr for Locale {
    type Err = anyhow::Error;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        let (rest, modifier) = match code.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (code, None),
        };
        let (language, encoding) = match rest.split_once('.') {
            Some((language, encoding)) => (language, Some(encoding)),
            None => (rest, None),
        };
        let part = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        };
        anyhow::ensure!(
            language.starts_with(|c: char| c.is_ascii_alphanumeric())
                && part(language)
                && encoding.is_none_or(part)
                && modifier.is_none_or(part),
            "Invalid locale code {code:?}: use letters, digits, `_` and `-`, with an optional \
             `.encoding` and `@modifier`"
        );
        Ok(Self(match modifier {
            Some(modifier) => format!("{language}@{modifier}"),
            None => language.to_owned(),
        }))
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Values by locale, sorted by locale so output is deterministic. Reading one
/// rejects invalid codes, and codes such as `en` and `en.UTF-8` that mean the
/// same locale.
#[derive(Clone, Debug)]
pub struct Locales<V>(BTreeMap<Locale, V>);

impl<V> Default for Locales<V> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Locales<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut locales = BTreeMap::new();
        let mut written = BTreeMap::new();
        for (code, value) in BTreeMap::<String, V>::deserialize(deserializer)? {
            let locale = code.parse::<Locale>().map_err(D::Error::custom)?;
            if let Some(other) = written.insert(locale.clone(), code.clone()) {
                return Err(D::Error::custom(format!(
                    "Locale codes {other:?} and {code:?} both mean \"{locale}\""
                )));
            }
            locales.insert(locale, value);
        }
        Ok(Self(locales))
    }
}

impl<V> Locales<V> {
    pub fn iter(&self) -> impl Iterator<Item = (&Locale, &V)> {
        self.0.iter()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.values()
    }

    pub fn get(&self, code: &str) -> Option<&V> {
        self.0.get(&code.parse::<Locale>().ok()?)
    }

    /// Add `later`, merging a locale both have with `merge(existing, later)`.
    pub fn overlay(&mut self, later: Self, merge: fn(&mut V, V)) {
        for (locale, value) in later.0 {
            match self.0.get_mut(&locale) {
                Some(existing) => merge(existing, value),
                None => {
                    self.0.insert(locale, value);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn insert(&mut self, code: impl AsRef<str>, value: V) {
        self.0.insert(code.as_ref().parse().unwrap(), value);
    }
}

#[cfg(test)]
impl<V> std::ops::Index<&str> for Locales<V> {
    type Output = V;

    fn index(&self, code: &str) -> &V {
        self.get(code).unwrap_or_else(|| panic!("no locale {code}"))
    }
}

/// Preferred fallback locales when a platform needs an unlocalized base value.
pub(crate) const FALLBACK_LOCALE_PREFERENCE: &[&str] = &["C", "en"];

#[cfg(test)]
mod tests {
    use super::*;

    fn read(codes: &[&str]) -> Result<Locales<()>, serde_json::Error> {
        let table: serde_json::Map<_, _> = codes
            .iter()
            .map(|code| (code.to_string(), serde_json::Value::Null))
            .collect();
        serde_json::from_value(table.into())
    }

    #[test]
    fn encodings_are_dropped_and_modifiers_kept() {
        let locales = read(&["de_DE.UTF-8@euro", "fr", "sr@latin"]).unwrap();
        let codes: Vec<_> = locales.iter().map(|(code, _)| code.to_string()).collect();
        assert_eq!(codes, ["de_DE@euro", "fr", "sr@latin"]);
        assert!(locales.get("fr.UTF-8").is_some());
    }

    #[test]
    fn unsafe_and_colliding_codes_are_rejected() {
        assert!(read(&["en", "fr_CA", "zh-Hans", "sr@latin"]).is_ok());
        for bad in [
            "", ".", "..", "../x", "a/b", "a\\b", "en US", "@euro", ".UTF-8", "en@", "en.",
            "a.b.c", "x@y@z", "_en",
        ] {
            assert!(read(&[bad]).is_err(), "{bad:?}");
        }
        let error = read(&["en", "en.UTF-8"]).unwrap_err();
        assert!(error.to_string().contains("both mean \"en\""), "{error}");
    }
}
