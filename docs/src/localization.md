# Localization

Translations of the display name and similar strings go in a `localizations`
table per platform, keyed by locale code:

```toml
[package.metadata.bundle.osx.localizations.fr]
CFBundleDisplayName = "Mon Application"

[package.metadata.bundle.ios.localizations.fr]
CFBundleDisplayName = "Mon Application"

[package.metadata.bundle.linux.localizations.fr]
Name = "Mon App"
Comment = "Une description"
```

| Platform | Result |
| --- | --- |
| macOS | `Contents/Resources/<locale>.lproj/InfoPlist.strings` |
| iOS | `<locale>.lproj/InfoPlist.strings` at the root of the app, sealed by the signature |
| Linux | `Name[fr]=` style keys in the `.desktop` file; see [Linux](platforms/linux.md) |

The strings files are written with sorted, quoted and escaped entries, so the
same settings always produce the same bytes and values may contain quotes,
backslashes and newlines.

## Translations in separate files

`localization_files` keeps translations out of `Cargo.toml`:

```toml
[package.metadata.bundle]
localization_files = ["locales/en.toml", "locales/fr.toml"]
```

Paths are relative to the package's `Cargo.toml`. Each file may hold tables for
several platforms, shaped like the inline ones (`macos` may be spelled `osx`):

```toml
[linux.fr]
Name = "Carte postale"
Keywords = ["carte", "courrier"]

[macos.fr]
CFBundleDisplayName = "Carte postale"

[ios.fr]
CFBundleDisplayName = "Carte postale"
```

Files are read in order, and a later value replaces the same key of an earlier
file; the other keys of that locale stay. Inline `localizations` in
`Cargo.toml` win over every file. Unknown platforms, misspelled Linux fields
and bad locale codes are errors in every table of every file, including those
for platforms you are not building. Because `localization_files` is a shared
setting, a platform table can replace it, and `localization_files = []` turns
it off for that platform. A named `bin` or `example` table needs its own list.

## Locale codes

A code is `language`, optionally followed by `.encoding` and `@modifier`, each
made of letters, digits, `_` and `-`: `fr`, `pt_BR`, `zh-Hans`, `sr@latin`. The
encoding (`fr.UTF-8`) is dropped and the modifier is kept. Anything else, such
as `../x`, `a/b`, `en@` or `a.b.c`, is rejected when settings are read, as is
a pair like `en` and `en.UTF-8` that would mean the same locale. The codes of
a desktop action's `NameLocalized` table follow the same rule.

## What is not localized

Windows executables and installers carry English metadata, and the APK label is
the bundle name. Application text is yours: ship your toolkit's catalogs
through [`resources`](resources.md) and test a language other than the default,
and a missing translation, in the installed app.

## Check the result

Open the packaged app from the platform launcher in each language and confirm
the name, icon and description still fit. Then complete the main task with the
keyboard alone, repeat it with the platform's screen reader, and look at larger
text, high contrast and a language with longer labels, including a
right-to-left one if you support it. Run these checks on the artifact you
distribute: bundled fonts, resources and missing libraries can change behavior
from a development build.
