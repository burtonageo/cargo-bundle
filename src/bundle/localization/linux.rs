//! FreeDesktop `.desktop` file localization.

use super::{FALLBACK_LOCALE_PREFERENCE, sorted_locales};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;

/// Keywords for a FreeDesktop `.desktop` entry (`localestring(s)`).
///
/// Accepts either a semicolon-separated string or a TOML list of strings.
/// Lists are joined with `;` and terminated with a trailing `;` per the
/// [Desktop Entry Spec](https://specifications.freedesktop.org/desktop-entry-spec/latest/).
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(untagged)]
pub enum DesktopKeywords {
    String(String),
    List(Vec<String>),
}

/// Per-locale FreeDesktop desktop-entry strings.
///
/// TOML keys use FreeDesktop names (`Name`, `GenericName`, `Comment`, `Keywords`)
/// so the manifest mirrors the generated desktop file. Only `localestring` keys
/// are supported here; non-localizable keys (`Exec`, `Icon`, …) stay global.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct LinuxDesktopLocale {
    #[serde(rename = "Name", default)]
    pub name: Option<String>,
    #[serde(rename = "GenericName", default)]
    pub generic_name: Option<String>,
    #[serde(rename = "Comment", default)]
    pub comment: Option<String>,
    #[serde(rename = "Keywords", default)]
    pub keywords: Option<DesktopKeywords>,
    /// Localized icon.
    #[serde(rename = "Icon", default)]
    pub icon: Option<String>,
}

/// Locale-keyed FreeDesktop desktop strings for Linux packages.
#[derive(Clone, Copy, Debug)]
pub struct LinuxDesktopLocalizations<'a> {
    locale_mapping: &'a HashMap<String, LinuxDesktopLocale>,
}

impl<'a> LinuxDesktopLocalizations<'a> {
    pub fn new(locale_mapping: &'a HashMap<String, LinuxDesktopLocale>) -> Self {
        Self { locale_mapping }
    }

    /// Locales sorted by code for deterministic artifact output.
    pub fn sorted_locales(&self) -> std::collections::BTreeMap<&'a str, &'a LinuxDesktopLocale> {
        sorted_locales(self.locale_mapping)
    }

    /// Look up a non-empty string field with `C` to `en` to first-sorted-locale fallback.
    pub fn fallback_string<F>(&self, extract_field: F) -> Option<&str>
    where
        F: Fn(&'a LinuxDesktopLocale) -> Option<&'a str>,
    {
        self.fallback(|locale_data| extract_field(locale_data).filter(|string| !string.is_empty()))
    }

    /// Generic fallback: walk C to en to first-sorted-locale, return first `Some`.
    fn fallback<T: ?Sized>(
        &self,
        extract_field: impl Fn(&'a LinuxDesktopLocale) -> Option<&'a T>,
    ) -> Option<&'a T> {
        FALLBACK_LOCALE_PREFERENCE
            .iter()
            .find_map(|preferred_locale| {
                self.locale_mapping
                    .get(*preferred_locale)
                    .and_then(&extract_field)
            })
            .or_else(move || {
                self.sorted_locales()
                    .values()
                    .find_map(|locale_data| extract_field(locale_data))
            })
    }

    /// Look up Keywords with the same fallback preference as string fields.
    pub fn fallback_keywords(&self) -> Option<&DesktopKeywords> {
        self.fallback(|locale_data| locale_data.keywords.as_ref())
    }

    /// Append FreeDesktop localized keys (`Name[fr]=…`, …) and any unlocalized
    /// `GenericName` / `Keywords` bases required by the Desktop Entry Spec.
    pub fn append_desktop_keys(&self, output_buffer: &mut String) {
        self.append_base_generic_name(output_buffer);
        self.append_base_keywords(output_buffer);
        self.append_localized_entries(output_buffer);
    }

    fn append_base_generic_name(&self, output_buffer: &mut String) {
        let has_localized_generic = self.locale_mapping.values().any(|locale_data| {
            locale_data
                .generic_name
                .as_ref()
                .is_some_and(|name| !name.is_empty())
        });

        if has_localized_generic
            && let Some(fallback_name) =
                self.fallback_string(|locale_data| locale_data.generic_name.as_deref())
        {
            let _ = writeln!(
                output_buffer,
                "GenericName={}",
                escape_desktop_value(fallback_name)
            );
        }
    }

    fn append_base_keywords(&self, output_buffer: &mut String) {
        let has_localized_keywords = self
            .locale_mapping
            .values()
            .any(|locale_data| locale_data.keywords.is_some());

        if has_localized_keywords && let Some(fallback_keywords) = self.fallback_keywords() {
            let _ = writeln!(
                output_buffer,
                "Keywords={}",
                format_keywords_value(fallback_keywords)
            );
        }
    }

    fn append_localized_entries(&self, output_buffer: &mut String) {
        for (locale_code, locale_data) in self.sorted_locales() {
            self.append_single_localized_entry(
                output_buffer,
                "Name",
                locale_code,
                locale_data.name.as_deref(),
            );
            self.append_single_localized_entry(
                output_buffer,
                "GenericName",
                locale_code,
                locale_data.generic_name.as_deref(),
            );
            self.append_single_localized_entry(
                output_buffer,
                "Comment",
                locale_code,
                locale_data.comment.as_deref(),
            );

            if let Some(keywords) = locale_data.keywords.as_ref() {
                let _ = writeln!(
                    output_buffer,
                    "Keywords[{locale_code}]={}",
                    format_keywords_value(keywords)
                );
            }

            self.append_single_localized_entry(
                output_buffer,
                "Icon",
                locale_code,
                locale_data.icon.as_deref(),
            );
        }
    }

    fn append_single_localized_entry(
        &self,
        output_buffer: &mut String,
        key: &str,
        locale_code: &str,
        value: Option<&str>,
    ) {
        if let Some(valid_string) = value.filter(|string| !string.is_empty()) {
            let _ = writeln!(
                output_buffer,
                "{key}[{locale_code}]={}",
                escape_desktop_value(valid_string)
            );
        }
    }

    /// Whether any locale provides a non-empty Comment.
    pub fn has_localized_comment(&self) -> bool {
        self.locale_mapping.values().any(|locale_data| {
            locale_data
                .comment
                .as_ref()
                .is_some_and(|comment| !comment.is_empty())
        })
    }

    /// Fallback Comment when the unlocalized base is empty but localizations exist.
    pub fn fallback_comment(&self) -> Option<&str> {
        self.fallback_string(|locale_data| locale_data.comment.as_deref())
    }
}

/// Escape a FreeDesktop desktop-entry value of type `string` / `localestring`.
pub fn escape_desktop_value(value: &str) -> Cow<'_, str> {
    if !value.contains(['\\', '\n', '\r', '\t']) {
        return Cow::Borrowed(value);
    }

    let mut escaped_string = String::with_capacity(value.len() + 4);
    for character in value.chars() {
        match character {
            '\\' => escaped_string.push_str(r"\\"),
            '\n' => escaped_string.push_str(r"\n"),
            '\r' => escaped_string.push_str(r"\r"),
            '\t' => escaped_string.push_str(r"\t"),
            _ => escaped_string.push(character),
        }
    }
    Cow::Owned(escaped_string)
}

/// Escape a single item inside a FreeDesktop `string(s)` / `localestring(s)` list.
fn escape_desktop_list_item(value: &str) -> Cow<'_, str> {
    let escaped_value = escape_desktop_value(value);
    if escaped_value.contains(';') {
        Cow::Owned(escaped_value.replace(';', r"\;"))
    } else {
        escaped_value
    }
}

/// Format a Keywords desktop value, applying escapes and standardizing structure.
pub fn format_keywords_value(keywords: &DesktopKeywords) -> String {
    match keywords {
        DesktopKeywords::String(keyword_string) => {
            let escaped_string = escape_desktop_value(keyword_string);
            if !escaped_string.is_empty() && !escaped_string.ends_with(';') {
                let mut string_with_semicolon = escaped_string.into_owned();
                string_with_semicolon.push(';');
                string_with_semicolon
            } else {
                escaped_string.into_owned()
            }
        }
        DesktopKeywords::List(keyword_list) => {
            let mut formatted_keywords = String::new();
            for item in keyword_list {
                formatted_keywords.push_str(&escape_desktop_list_item(item));
                formatted_keywords.push(';');
            }
            formatted_keywords
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_desktop_value_escapes_specials() {
        assert_eq!(escape_desktop_value(r"a\b"), r"a\\b");
        assert_eq!(escape_desktop_value("line\nbreak"), r"line\nbreak");
        assert_eq!(escape_desktop_value("tab\there"), r"tab\there");
        assert_eq!(escape_desktop_value("ret\rurn"), r"ret\rurn");
    }

    #[test]
    fn append_desktop_keys_sorted_locales() {
        let mut locale_mapping = HashMap::new();
        locale_mapping.insert(
            "fr".to_string(),
            LinuxDesktopLocale {
                icon: None,
                name: Some("Mon App".into()),
                comment: Some("Une description".into()),
                generic_name: Some("Utilitaire".into()),
                keywords: Some(DesktopKeywords::List(vec![
                    "outil".into(),
                    "utilitaire".into(),
                ])),
            },
        );
        locale_mapping.insert(
            "de".to_string(),
            LinuxDesktopLocale {
                icon: None,
                name: Some("Meine App".into()),
                comment: Some("Eine Beschreibung".into()),
                generic_name: Some("Dienstprogramm".into()),
                keywords: Some(DesktopKeywords::String("werkzeug;dienstprogramm".into())),
            },
        );

        let localizations = LinuxDesktopLocalizations::new(&locale_mapping);
        let mut output_buffer = String::new();
        localizations.append_desktop_keys(&mut output_buffer);

        assert!(output_buffer.contains("GenericName=Dienstprogramm\n"));
        assert!(output_buffer.contains("Keywords=werkzeug;dienstprogramm;\n"));
        assert!(output_buffer.contains("Name[de]=Meine App\n"));
        assert!(output_buffer.contains("Name[fr]=Mon App\n"));

        let de_position = output_buffer.find("Name[de]=").unwrap();
        let fr_position = output_buffer.find("Name[fr]=").unwrap();
        assert!(de_position < fr_position);
    }

    #[test]
    fn fallback_prefers_en_over_first_sorted() {
        let mut locale_mapping = HashMap::new();
        locale_mapping.insert(
            "fr".to_string(),
            LinuxDesktopLocale {
                icon: None,
                generic_name: Some("Utilitaire".into()),
                ..Default::default()
            },
        );
        locale_mapping.insert(
            "en".to_string(),
            LinuxDesktopLocale {
                icon: None,
                generic_name: Some("Utility".into()),
                ..Default::default()
            },
        );
        let localizations = LinuxDesktopLocalizations::new(&locale_mapping);
        assert_eq!(
            localizations.fallback_string(|locale_data| locale_data.generic_name.as_deref()),
            Some("Utility")
        );
    }

    #[test]
    fn keywords_escape_embedded_semicolons() {
        let keywords = DesktopKeywords::List(vec!["a;b".into(), "c".into()]);
        assert_eq!(format_keywords_value(&keywords), r"a\;b;c;");
    }
}
