//! FreeDesktop `.desktop` Application entry generation.

use crate::bundle::localization::{LinuxDesktopLocalizations, escape_desktop_value};
use crate::bundle::settings::DesktopAction;
use crate::bundle::{Settings, common};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;

/// Per-package-format options for `.desktop` generation that don't come from
/// bundle settings.
#[derive(Debug, Default)]
pub struct DesktopFileOptions {
    /// Available so appimaged/AppImageLauncher can
    /// distinguish integrated versions
    pub appimage_version: Option<String>,
}

/// Inputs needed to render a FreeDesktop `.desktop` Application entry.
/// Kept separate from [`Settings`] so unit tests need not construct a full
/// Cargo package.
#[derive(Debug)]
struct DesktopEntryInput<'a> {
    bin_name: &'a str,
    app_name: &'a str,
    comment: &'a str,
    categories: Option<&'a str>,
    exec_args: Option<&'a str>,
    use_terminal: bool,
    mime_types: &'a [String],
    localizations: Option<LinuxDesktopLocalizations<'a>>,
    startup_wm_class: Option<&'a str>,
    appimage_version: Option<&'a str>,
    actions: Option<&'a HashMap<String, DesktopAction>>,
}

/// Render a FreeDesktop `.desktop` Application entry as a string.
fn render_desktop_entry(input: &DesktopEntryInput<'_>) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();

    // For more information about the format of this file, see
    // https://specifications.freedesktop.org/desktop-entry-spec/latest/
    //
    // Encoding=UTF-8 is omitted: the key is deprecated (files are UTF-8 by
    // definition in modern Desktop Entry Spec versions).
    writeln!(out, "[Desktop Entry]").unwrap();
    if let Some(categories) = input.categories {
        writeln!(out, "Categories={categories}").unwrap();
    }

    // Comment: unlocalized base is required if any Comment[locale] is present.
    let needs_comment_fallback = input
        .localizations
        .as_ref()
        .is_some_and(|l| l.has_localized_comment());

    if !input.comment.is_empty() {
        writeln!(out, "Comment={}", escape_desktop_value(input.comment)).unwrap();
    } else if needs_comment_fallback
        && let Some(fallback) = input
            .localizations
            .as_ref()
            .and_then(|l| l.fallback_comment())
    {
        writeln!(out, "Comment={}", escape_desktop_value(fallback)).unwrap();
    }

    match input.exec_args {
        Some(args) => writeln!(out, "Exec={} {}", input.bin_name, args).unwrap(),
        None => writeln!(out, "Exec={}", input.bin_name).unwrap(),
    }

    writeln!(out, "Icon={}", input.bin_name).unwrap();
    writeln!(out, "Name={}", escape_desktop_value(input.app_name)).unwrap();

    // GenericName / Keywords bases + all Key[locale]=… lines.
    if let Some(locs) = &input.localizations {
        locs.append_desktop_keys(&mut out);
    }

    writeln!(out, "Terminal={}", input.use_terminal).unwrap();
    writeln!(out, "Type=Application").unwrap();

    if let Some(wm_class) = input.startup_wm_class {
        writeln!(out, "StartupWMClass={}", escape_desktop_value(wm_class)).unwrap();
    }

    if let Some(version) = input.appimage_version {
        writeln!(out, "X-AppImage-Version={}", escape_desktop_value(version)).unwrap();
    }

    // Omit empty MimeType; a bare `MimeType=` is noise and fails some validators.
    if !input.mime_types.is_empty() {
        write!(out, "MimeType=").unwrap();
        for mime in input.mime_types {
            write!(out, "{mime};").unwrap();
        }
        writeln!(out).unwrap();
    }

    // Desktop Actions: `Actions=` list in the main group (sorted for stable
    // diffs), then one `[Desktop Action <id>]` group per action.
    let sorted_actions: BTreeMap<&str, &DesktopAction> = input
        .actions
        .into_iter()
        .flatten()
        .map(|(id, action)| (id.as_str(), action))
        .collect();

    if !sorted_actions.is_empty() {
        write!(out, "Actions=").unwrap();
        for id in sorted_actions.keys() {
            write!(out, "{id};").unwrap();
        }
        writeln!(out).unwrap();
    }

    for (id, action) in &sorted_actions {
        writeln!(out).unwrap();
        writeln!(out, "[Desktop Action {id}]").unwrap();
        writeln!(out, "Name={}", escape_desktop_value(&action.name)).unwrap();
        if let Some(localized) = &action.name_localized {
            let sorted: BTreeMap<&str, &String> = localized
                .iter()
                .map(|(locale, name)| (locale.as_str(), name))
                .collect();
            for (locale, name) in sorted {
                writeln!(out, "Name[{locale}]={}", escape_desktop_value(name)).unwrap();
            }
        }
        let exec = action.exec.as_deref().unwrap_or(input.bin_name);
        writeln!(out, "Exec={exec}").unwrap();
        if let Some(icon) = &action.icon {
            writeln!(out, "Icon={}", escape_desktop_value(icon)).unwrap();
        }
    }

    // The `Version` field is omitted on purpose. See `generate_control_file` for
    // specifying the application version (Desktop Entry `Version` is the spec
    // version, not the app version).
    out
}

/// Generate the application desktop file and store it under the `data_dir`.
pub fn generate_desktop_file(
    settings: &Settings,
    data_dir: &Path,
    options: &DesktopFileOptions,
) -> crate::Result<()> {
    let bin_name = settings.binary_name();
    let desktop_file_name = format!("{bin_name}.desktop");
    let desktop_file_path = data_dir
        .join("usr/share/applications")
        .join(desktop_file_name);
    let file = &mut common::create_file(&desktop_file_path)?;

    // desktop-file-validate and appimagetool both expect a Categories key;
    // default to Utility when the manifest doesn't set one.
    let categories = match settings.app_category() {
        Some(category) => category.gnome_desktop_categories().to_string(),
        None => {
            let _ = common::print_warning(
                "No category set in bundle settings; defaulting to Categories=Utility; \
                 in the .desktop file. Set `category` in [package.metadata.bundle] to \
                 pick a proper one.",
            );
            "Utility;".to_string()
        }
    };

    let contents = render_desktop_entry(&DesktopEntryInput {
        bin_name,
        app_name: settings.bundle_name(),
        comment: settings.short_description(),
        categories: Some(&categories),
        exec_args: settings.linux_exec_args(),
        use_terminal: settings.linux_use_terminal().unwrap_or(false),
        mime_types: settings.linux_mime_types(),
        localizations: settings.linux_localizations(),
        startup_wm_class: settings.linux_startup_wm_class(),
        appimage_version: options.appimage_version.as_deref(),
        actions: settings.linux_desktop_actions(),
    });

    file.write_all(contents.as_bytes())?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::localization::{DesktopKeywords, LinuxDesktopLocale};
    use std::collections::HashMap;

    #[test]
    fn desktop_entry_basic_without_localizations() {
        let mime = vec!["text/plain".to_string()];
        let contents = render_desktop_entry(&DesktopEntryInput {
            bin_name: "myapp",
            app_name: "My App",
            comment: "A short description",
            categories: Some("Utility;"),
            exec_args: Some("%f"),
            use_terminal: false,
            mime_types: &mime,
            localizations: None,
            startup_wm_class: None,
            appimage_version: None,
            actions: None,
        });

        assert!(contents.starts_with("[Desktop Entry]\n"));
        assert!(!contents.contains("Encoding="));
        assert!(contents.contains("Categories=Utility;\n"));
        assert!(contents.contains("Comment=A short description\n"));
        assert!(contents.contains("Exec=myapp %f\n"));
        assert!(contents.contains("Icon=myapp\n"));
        assert!(contents.contains("Name=My App\n"));
        assert!(contents.contains("Terminal=false\n"));
        assert!(contents.contains("Type=Application\n"));
        assert!(contents.contains("MimeType=text/plain;\n"));
        assert!(!contents.contains("Name["));
    }

    #[test]
    fn desktop_entry_omits_empty_mime_type() {
        let contents = render_desktop_entry(&DesktopEntryInput {
            bin_name: "myapp",
            app_name: "My App",
            comment: "",
            categories: None,
            exec_args: None,
            use_terminal: true,
            mime_types: &[],
            localizations: None,
            startup_wm_class: None,
            appimage_version: None,
            actions: None,
        });
        assert!(!contents.contains("MimeType"));
        assert!(contents.contains("Terminal=true\n"));
        assert!(!contents.contains("Comment="));
    }

    #[test]
    fn desktop_entry_with_multiple_locales() {
        let mut locs = HashMap::new();
        locs.insert(
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
        locs.insert(
            "de".to_string(),
            LinuxDesktopLocale {
                icon: None,
                name: Some("Meine App".into()),
                comment: Some("Eine Beschreibung".into()),
                generic_name: Some("Dienstprogramm".into()),
                keywords: Some(DesktopKeywords::String("werkzeug;dienstprogramm".into())),
            },
        );
        locs.insert(
            "pt_BR".to_string(),
            LinuxDesktopLocale {
                icon: None,
                name: Some("Meu App".into()),
                comment: Some("Uma descrição".into()),
                generic_name: None,
                keywords: None,
            },
        );

        let contents = render_desktop_entry(&DesktopEntryInput {
            bin_name: "myapp",
            app_name: "My App",
            comment: "A short description",
            categories: Some("Utility;"),
            exec_args: None,
            use_terminal: false,
            mime_types: &[],
            localizations: Some(LinuxDesktopLocalizations::new(&locs)),
            startup_wm_class: None,
            appimage_version: None,
            actions: None,
        });

        assert!(contents.contains("Name=My App\n"));
        assert!(contents.contains("Comment=A short description\n"));
        assert!(contents.contains("GenericName=Dienstprogramm\n"));
        assert!(contents.contains("Keywords=werkzeug;dienstprogramm;\n"));
        assert!(contents.contains("Name[de]=Meine App\n"));
        assert!(contents.contains("Name[fr]=Mon App\n"));
        assert!(contents.contains("Name[pt_BR]=Meu App\n"));
        assert!(!contents.contains("GenericName[pt_BR]"));

        let de_pos = contents.find("Name[de]=").unwrap();
        let fr_pos = contents.find("Name[fr]=").unwrap();
        let pt_pos = contents.find("Name[pt_BR]=").unwrap();
        assert!(de_pos < fr_pos && fr_pos < pt_pos);
    }

    #[test]
    fn desktop_entry_prefers_en_for_unlocalized_generic_name() {
        let mut locs = HashMap::new();
        locs.insert(
            "fr".to_string(),
            LinuxDesktopLocale {
                icon: None,
                name: Some("Mon App".into()),
                generic_name: Some("Utilitaire".into()),
                comment: None,
                keywords: None,
            },
        );
        locs.insert(
            "en".to_string(),
            LinuxDesktopLocale {
                icon: None,
                name: Some("My App".into()),
                generic_name: Some("Utility".into()),
                comment: None,
                keywords: None,
            },
        );

        let contents = render_desktop_entry(&DesktopEntryInput {
            bin_name: "myapp",
            app_name: "My App",
            comment: "",
            categories: None,
            exec_args: None,
            use_terminal: false,
            mime_types: &[],
            localizations: Some(LinuxDesktopLocalizations::new(&locs)),
            startup_wm_class: None,
            appimage_version: None,
            actions: None,
        });

        assert!(contents.contains("GenericName=Utility\n"));
        assert!(contents.contains("GenericName[en]=Utility\n"));
        assert!(contents.contains("GenericName[fr]=Utilitaire\n"));
    }

    #[test]
    fn desktop_entry_escapes_values_and_keyword_semicolons() {
        let mut locs = HashMap::new();
        locs.insert(
            "fr".to_string(),
            LinuxDesktopLocale {
                icon: None,
                name: Some("App\\Name".into()),
                comment: Some("line1\nline2".into()),
                generic_name: None,
                keywords: Some(DesktopKeywords::List(vec!["a;b".into(), "c".into()])),
            },
        );

        let contents = render_desktop_entry(&DesktopEntryInput {
            bin_name: "myapp",
            app_name: "Name\\With\\Backslash",
            comment: "has\ttab",
            categories: None,
            exec_args: None,
            use_terminal: false,
            mime_types: &[],
            localizations: Some(LinuxDesktopLocalizations::new(&locs)),
            startup_wm_class: None,
            appimage_version: None,
            actions: None,
        });

        assert!(contents.contains(r"Name=Name\\With\\Backslash"));
        assert!(contents.contains(r"Comment=has\ttab"));
        assert!(contents.contains(r"Name[fr]=App\\Name"));
        assert!(contents.contains(r"Comment[fr]=line1\nline2"));
        assert!(contents.contains(r"Keywords[fr]=a\;b;c;"));
    }

    #[test]
    fn locales_are_sorted() {
        let mut locs = HashMap::new();
        locs.insert("fr".to_string(), LinuxDesktopLocale::default());
        locs.insert("de".to_string(), LinuxDesktopLocale::default());
        let table = LinuxDesktopLocalizations::new(&locs);
        let sorted: Vec<_> = table.sorted_locales().keys().copied().collect();
        assert_eq!(sorted, vec!["de", "fr"]);
    }

    #[test]
    fn desktop_entry_wm_class_version_and_actions() {
        let mut name_localized = HashMap::new();
        name_localized.insert("fr".to_string(), "Nouvelle fenetre".to_string());

        let mut actions = HashMap::new();
        actions.insert(
            "new-window".to_string(),
            DesktopAction {
                name: "New Window".to_string(),
                exec: Some("myapp --new-window".to_string()),
                icon: Some("myapp-new".to_string()),
                name_localized: Some(name_localized),
            },
        );
        actions.insert(
            "about".to_string(),
            DesktopAction {
                name: "About".to_string(),
                exec: None,
                icon: None,
                name_localized: None,
            },
        );

        let contents = render_desktop_entry(&DesktopEntryInput {
            bin_name: "myapp",
            app_name: "My App",
            comment: "",
            categories: Some("Utility;"),
            exec_args: None,
            use_terminal: false,
            mime_types: &[],
            localizations: None,
            startup_wm_class: Some("myapp-window"),
            appimage_version: Some("1.2.3"),
            actions: Some(&actions),
        });

        assert!(contents.contains("StartupWMClass=myapp-window\n"));
        assert!(contents.contains("X-AppImage-Version=1.2.3\n"));
        assert!(contents.contains("Actions=about;new-window;\n"));
        assert!(contents.contains("[Desktop Action about]\n"));
        assert!(contents.contains("[Desktop Action new-window]\n"));
        assert!(contents.contains("Name=New Window\n"));
        assert!(contents.contains("Name[fr]=Nouvelle fenetre\n"));
        assert!(contents.contains("Exec=myapp --new-window\n"));
        assert!(contents.contains("Icon=myapp-new\n"));

        // Action with no Exec falls back to the binary.
        let about_group = contents.split("[Desktop Action about]").nth(1).unwrap();
        assert!(about_group.contains("Exec=myapp\n"));

        // The Actions list appears in the main group, before the action groups.
        assert!(contents.find("Actions=").unwrap() < contents.find("[Desktop Action").unwrap());
    }

    #[test]
    fn locale_encoding_part_is_stripped() {
        let mut locs = HashMap::new();
        locs.insert(
            "fr.UTF-8".to_string(),
            LinuxDesktopLocale {
                icon: Some("myapp-fr".into()),
                name: Some("Mon App".into()),
                comment: None,
                generic_name: None,
                keywords: None,
            },
        );

        let contents = render_desktop_entry(&DesktopEntryInput {
            bin_name: "myapp",
            app_name: "My App",
            comment: "",
            categories: None,
            exec_args: None,
            use_terminal: false,
            mime_types: &[],
            localizations: Some(LinuxDesktopLocalizations::new(&locs)),
            startup_wm_class: None,
            appimage_version: None,
            actions: None,
        });

        assert!(contents.contains("Name[fr]=Mon App\n"));
        assert!(contents.contains("Icon[fr]=myapp-fr\n"));
        assert!(!contents.contains("fr.UTF-8"));
    }
}
