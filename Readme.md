# Cargo bundle

[![Crates.io](https://img.shields.io/crates/v/cargo-bundle.svg)](https://crates.io/crates/cargo-bundle)
[![Build Status](https://github.com/burtonageo/cargo-bundle/workflows/CI/badge.svg?branch=master)](https://github.com/burtonageo/cargo-bundle/actions?query=branch%3Amaster)

Wrap Rust executables in OS-specific app bundles

## About

`cargo-bundle` is a tool used to generate installers or app bundles for GUI
executables built with `cargo`.  It can create `.app` bundles for Mac OS X and
iOS, `.deb` packages and `.AppImage` bundles for Linux, and `.msi` installers
for Windows (note however that iOS and Windows support is still experimental).
Support for creating `.rpm` packages (for Linux) and `.apk` packages (for
Android) is still pending.

To install `cargo bundle`, run `cargo install cargo-bundle`. This will add the most recent version of `cargo-bundle`
published to [crates.io](https://crates.io/crates/cargo-bundle) as a subcommand to your default `cargo` installation.

To start using `cargo bundle`, add a `[package.metadata.bundle]` section to your project's `Cargo.toml` file.  This
section describes various attributes of the generated bundle, such as its name, icon, description, copyright, as well
as any packaging scripts you need to generate extra data.  The full manifest format is described below.

To build a bundle for the OS you're on, simply run `cargo bundle` in your
project's directory (where the `Cargo.toml` is placed).  If you would like to
bundle a release build, you must add the `--release` flag to your call.  To
cross-compile and bundle an application for another OS, add an appropriate
`--target` flag, just as you would for `cargo build`.  On macOS the
`--target` flag may be repeated to build every architecture and combine them
into a single universal binary with `lipo`, e.g.
`cargo bundle -t aarch64-apple-darwin -t x86_64-apple-darwin`.

If the executable has already been built, and you'd like to avoid rebuilding,
or perhaps maintain a change you made against it, pass it with `--binary-path` to
package it without running `cargo build` again. This is useful when a CI build
and packaging step are separate, or when another build system produced the
executable. Cargo metadata is still read for the bundle manifest and selected
`--bin`/`--example`; `--release`, `--profile`, and `--target` still select the
bundle output directory and target metadata, but do not rebuild the supplied
file.

```bash
cargo build --release --bin my-app
cargo bundle --release --bin my-app --binary-path target/release/my-app
```

The supplied path must be a regular file. It is copied into the generated
bundle; the input binary is not modified.

## Flags
  ```plaintext
  -b, --bin <NAME>           Bundle the specified binary
  -e, --example <NAME>       Bundle the specified example
  -f, --format <FORMAT>      Which bundle format to produce [possible values: deb, ios, msi, wxsmsi, osx, rpm, appimage]
  -r, --release              Build a bundle from a target built in release mode
      --profile <NAME>       Build a bundle from a target build using the given profile
      --binary-path <PATH>   Bundle this already-built executable instead of running `cargo build`
  -t, --target <TRIPLE>      Build a bundle for the target triple. May be repeated to combine several architectures into a universal binary (macOS only)
      --features <FEATURES>  Set crate features for the bundle. Eg: `--features "f1 f2"`
      --all-features         Build a bundle with all crate features
      --no-default-features  Build a bundle without the default crate features
  -p, --package <SPEC>       The name of the package to bundle. If not specified, the root package will be used
  -h, --help                 Print help
  -V, --version              Print version
  ```

## Targets
  ```bash
  aarch64-unknown-linux-gnu     ARM64 Linux (kernel 4.1, glibc 2.17+) 1
  i686-pc-windows-gnu           32-bit MinGW (Windows 7+) 2 3
  i686-pc-windows-msvc          32-bit MSVC (Windows 7+) 2 3
  i686-unknown-linux-gnu        32-bit Linux (kernel 3.2+, glibc 2.17+) 3
  x86_64-apple-darwin           64-bit macOS (10.12+, Sierra+)
  x86_64-pc-windows-gnu         64-bit MinGW (Windows 7+) 2
  x86_64-pc-windows-msvc        64-bit MSVC (Windows 7+) 2
  x86_64-unknown-linux-gnu      64-bit Linux (kernel 3.2+, glibc 2.17+)
  ```

## Bundle manifest format

There are several fields in the `[package.metadata.bundle]` section.


### General settings

These settings apply to bundles for all (or most) OSes.

 * `name`: The name of the built application. If this is not present, then it will use the `name` value from `bin`
           target in your `Cargo.toml` file.
 * `identifier`: [**REQUIRED**] A string that uniquely identifies your application,
   in reverse-DNS form (for example, `"com.example.appname"` or
   `"io.github.username.project"`).  For OS X and iOS, this is used as the
   bundle's `CFBundleIdentifier` value; for Windows, this is hashed to create
   an application GUID.
 * `icon`: [OPTIONAL] The icons used for your application.  This should be an array of file paths or globs (with images
           in various sizes/formats); `cargo-bundle` will automatically convert between image formats as necessary for
           different platforms.  Supported formats include SVG (Linux only), ICNS, ICO, PNG, and anything else that can be decoded by the
           [`image`](https://crates.io/crates/image) crate.  Icons intended for high-resolution (e.g. [Retina](https://developer.apple.com/design/human-interface-guidelines/app-icons#macOS-app-icon-sizes)) displays
           should have a filename with `@2x` just before the extension (see example below).
 * `version`: [OPTIONAL] The version of the application. If this is not present, then it will use the `version`
              value from your `Cargo.toml` file.
 * `resources`: [OPTIONAL] List of files or directories which will be copied to the resources section of the
                bundle. Globs are supported.
 * `script`: [OPTIONAL] This is a reserved field; at the moment it is not used for anything, but may be used to
             run scripts while packaging the bundle (e.g. download files, compress and encrypt, etc.).
 * `copyright`: [OPTIONAL] This contains a copyright string associated with your application.
 * `category`: [OPTIONAL] What kind of application this is.  This can
   be a human-readable string (e.g. `"Puzzle game"`), or a Mac OS X
   [LSApplicationCategoryType](https://developer.apple.com/documentation/bundleresources/information_property_list/lsapplicationcategorytype#possibleValues) value
   (e.g. `"public.app-category.puzzle-games"`), or a GNOME desktop
   file category name (e.g. `"LogicGame"`), and `cargo-bundle` will
   automatically convert as needed for different platforms.
 * `short_description`: [OPTIONAL] A short, one-line description of the application. If this is not present, then it
                        will use the `description` value from your `Cargo.toml` file.
 * `long_description`: [OPTIONAL] A longer, multi-line description of the application.

note: `description` is also **required** in the `[package]` section.

### Platform-specific settings

Platform-specific settings belong in their platform sub-table. This keeps the
metadata typed: for example, macOS frameworks cannot be declared in the main
bundle table.

```toml
[package.metadata.bundle]
name = "Example"
identifier = "com.example.app"

[package.metadata.bundle.osx]
frameworks = ["WebKit.framework"]
```

### Linux-specific settings

These settings are used when bundling Linux packages (`deb`, `rpm`, `appimage`).

Declare these in `[package.metadata.bundle.linux]` using the concise names below.

* `mime_types`: A list of strings which represent mime types. If present, these are assigned
  to the `MimeType` field of the .desktop file.
* `exec_args`: A single string which is inserted after the name of the binary in the `Exec`
  field in the `.desktop` file. For example if the binary is called `my_program` and
  `exec_args = "%f"` then the Exec filed will be `Exec=my_program %f`. Find out more from the
  [specification](https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html#exec-variables)
* `use_terminal`: A boolean variable indicating the app is a console app or a gui app, default it's set to false.
* `localizations`: Per-locale translations for FreeDesktop `.desktop` entry fields.
  Mirrors the shape of `osx.localizations`: each sub-table is a locale code
  (`fr`, `de`, `pt_BR`, `zh_CN`, …) containing optional FreeDesktop
  [localestring](https://specifications.freedesktop.org/desktop-entry-spec/latest/recognized-keys.html)
  keys. Supported keys:

  * `Name` -- translated application name (`Name[fr]=…`)
  * `GenericName` -- generic type name, e.g. "Web Browser"
  * `Comment` -- short description / tooltip
  * `Keywords` -- search keywords; either a semicolon-separated string
    (`"outil;utilitaire"`) or a TOML list (`["outil", "utilitaire"]`), both
    emitted with a trailing `;` per the Desktop Entry Spec
  * `Icon` — locale-specific icon name (`Icon[locale]=…`; rarely needed)

  Locale codes may include an encoding part (`fr.UTF-8`); it is stripped when
  rendering, since desktop files are always UTF-8.

  Unlocalized `Name` and `Comment` still come from `name` / `short_description`
  (or the package description). If only localized `GenericName` / `Keywords`
  are provided, the unlocalized base value is taken from the `C` or `en`
  locale when present, otherwise from the first locale in sorted order
  (FreeDesktop requires an unlocalized key whenever any `Key[locale]` is
  present).

  Example:

  ```toml
  [package.metadata.bundle.linux.localizations.fr]
  Name = "Mon App"
  Comment = "Une description"
  GenericName = "Utilitaire"
  Keywords = ["outil", "utilitaire"]

  [package.metadata.bundle.linux.localizations.de]
  Name = "Meine App"
  Comment = "Eine Beschreibung"
  Keywords = "werkzeug;dienstprogramm"
  ```
* `startup_wm_class`: [OPTIONAL] Value for the `StartupWMClass` key of the `.desktop`
  file, used by desktop environments to match running windows to the launcher entry.
* `desktop_actions`: [OPTIONAL] Additional application actions (e.g. "New Window")
  shown in launcher context menus, emitted as `[Desktop Action <id>]` groups. Each sub-table
  key is an action id; `Name` is required, `Exec` defaults to the app binary, `Icon` and
  per-locale `NameLocalized` are optional:

  ```toml
  [package.metadata.bundle.linux.desktop_actions.new-window]
  Name = "New Window"
  Exec = "myapp --new-window"

  [package.metadata.bundle.linux.desktop_actions.new-window.NameLocalized]
  fr = "Nouvelle fenêtre"
  ```

### AppImage-specific settings

These settings are used only when bundling `appimage` packages.

* `appimage_runtime_path`: Path to the local type-2 AppImage runtime ELF. `cargo-bundle` never
  downloads runtimes; this makes AppImage builds reproducible and usable in offline CI. The file
  must be a valid ELF binary.
* `appimage_metainfo_path`: [OPTIONAL] Path to an
  [AppStream metainfo](https://www.freedesktop.org/software/appstream/docs/) XML file, copied to
  `usr/share/metainfo/<identifier>.appdata.xml` inside the AppDir. A warning is printed when
  absent — software centers increasingly expect metainfo.
* `appimage_compression`: [OPTIONAL] SquashFS codec, `"gzip"` (default, maximally compatible),
  `"lz4"`, `"lzo"`, or `"none"`.

**Limitations:** Shared libraries are **not** auto-collected (unlike tools that wrap
`linuxdeploy` / `appimagetool`). Dynamically linked GUI apps may need matching system libraries
on the target machine, or you can stage libs under `usr/lib` in the AppDir yourself. 

### Debian-specific settings

These settings are used only when bundling `deb` packages.

* `deb_depends`: A list of strings indicating other packages (e.g. shared
  libraries) that this package depends on to be installed.  If present, this
  forms the `Depends:` field of the `deb` package control file.

### Mac OS X-specific settings

These settings are used only when bundling `osx` packages and belong in
`[package.metadata.bundle.osx]`.

* `frameworks`: A list of strings indicating any Mac OS X frameworks that
  need to be bundled with the app.  Each string can either be the name of a
  framework (without the `.framework` extension, e.g. `"SDL2"`), in which case
  `cargo-bundle` will search for that framework in the standard install
  locations (`~/Library/Frameworks/`, `/Library/Frameworks/`, and
  `/Network/Library/Frameworks/`), or a path to a specific framework bundle
  (e.g. `./data/frameworks/SDL2.framework`).  Note that this setting just makes
  `cargo-bundle` copy the specified frameworks into the OS X app bundle (under
  `Foobar.app/Contents/Frameworks/`); you are still responsible for (1)
  arranging for the compiled binary to link against those frameworks (e.g. by
  emitting lines like `cargo:rustc-link-lib=framework=SDL2` from your
  `build.rs` script), and (2) embedding the correct rpath in your binary
  (e.g. by running `install_name_tool -add_rpath
  "@executable_path/../Frameworks" path/to/binary` after compiling).
* `minimum_system_version`: A version string indicating the minimum Mac OS
  X version that the bundled app supports (e.g. `"10.11"`).  If you are using
  this config field, you may also want have your `build.rs` script emit
  `cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=10.11` (or whatever version number
  you want) to ensure that the compiled binary has the same minimum version.
* `url_schemes`: A list of strings indicating the URL schemes that the app
  handles.
* `info_plist_exts`: A list of path strings that contain extra values for
  `Info.plist`. It reads each file in that path, and blindly appends its
  contents into the `Info.plist` file, after cargo-bundle has generated its
  keys but before it closes the `<dict>` and `<plist>`.
* `dmg_background`: A path string pointing to an SVG that becomes the
  Finder window background of the DMG bundle.  The SVG must contain elements
  with ids `app` and `applications`; the center of each element becomes the
  icon position of the application bundle and of the `/Applications` symlink
  respectively.  The image is rendered at 2x resolution so it stays crisp on
  Retina displays.  See `examples/hello/dmg-background.svg` for an example.

### Code signing

Apple application bundles and DMGs are signed in-process with the pure-Rust
[`apple-codesign`](https://crates.io/crates/apple-codesign) library. Export the
Developer ID certificate and private key from Keychain Access as a `.p12` file,
then configure it by path. The password is read from an environment variable;
optional entitlements, hardened runtime, and an RFC 3161 timestamp are also
supported:

```toml
[package.metadata.bundle]
apple_signing_p12 = "secrets/developer-id.p12"
apple_signing_password_env = "APPLE_SIGNING_PASSWORD"
apple_signing_entitlements = "packaging/entitlements.plist"
apple_signing_hardened_runtime = true
apple_signing_timestamp_url = "https://timestamp.example.com"
```

The same implementation works on Linux, Windows, and macOS; no `codesign` or
other signing executable is spawned. It signs the finished `.app` before DMG
creation, and then signs the final DMG itself. It does not notarize artifacts.
Do not modify a signed app or DMG afterwards: changing an executable,
framework, plugin, resource, or disk image invalidates its signature. Keep the
`.p12` file and its password in CI secrets, never in version control.

Windows `.exe` and `.msi` artifacts can be Authenticode-signed with a PKCS#12
certificate. This support is **not enabled by default** because it links the
vendored `osslsigncode` implementation, which is GPL-3.0-or-later (with an
OpenSSL linking exception). Build or install cargo-bundle with the explicit
feature only if those terms are acceptable for the binary you distribute:

```bash
cargo install cargo-bundle --features windows-signing
```

Then keep the certificate password out of `Cargo.toml` and configure signing
through an environment variable:

```toml
[package.metadata.bundle.windows_signing]
certificate_path = "secrets/publisher.p12"
certificate_password_env = "WINDOWS_CERTIFICATE_PASSWORD"
timestamp_url = "https://timestamp.example.com"
```

`timestamp_url` is optional and is sent as an RFC 3161 timestamp request. When
`certificate_password_env` is omitted, the signing backend prompts on the
terminal. Supplying Windows signing metadata to a cargo-bundle executable that
was not built with `windows-signing` is an error rather than silently emitting
an unsigned artifact. In CI, protect the certificate and password as secrets;
never commit either one. Signatures are applied to the final generated `.exe`
or `.msi`, so later changes invalidate them.

Linux artifacts use keyless [Sigstore](https://www.sigstore.dev/) signing when configured.

Configure the environment variable holding an OIDC identity token whose
audience is `sigstore`:

```toml
[package.metadata.bundle.linux_signing]
identity_token_env = "SIGSTORE_ID_TOKEN"
```

Every generated `.deb`, `.rpm`, or `.AppImage` then receives an adjacent
`<artifact>.sigstore.json` Sigstore bundle containing the signature,
certificate, and transparency-log proof. The original artifact is unchanged;
publish the sidecar bundle with it. This is keyless signing: the token must be
fresh and normally comes from your CI provider's OIDC integration. For a
release that signs both Linux and Windows outputs, install with
`--features windows-signing`; enabling `windows-signing` still has the
GPL-3.0-or-later consequence described above.

* note: Github Actions and Bitbucket Pipelines both have Apple MacOS build runners/containers available to use for free 

### Settings for specified binary

`[package.metadata.bundle]` only applies to the main executable.
Settings for other binaries can be specified in a `[package.metadata.bundle.bin.<binary name>]` section.

```toml
[package]
# other fields...

[package.metadata.bundle.bin.foo]
icon = ["icons/foo32x32.png"]
# other fields...

[[bin]]
name = "foo"
path = "src/bin/foo.rs"
# other fields...
```

### Example `Cargo.toml`:

```toml
[package]
name = "example"
# ...other fields...

[package.metadata.bundle]
name = "ExampleApplication"
identifier = "com.doe.exampleapplication"
icon = ["32x32.png", "128x128.png", "128x128@2x.png"]
version = "1.0.0"
resources = ["assets", "images/**/*.png", "secrets/public_key.txt"]
copyright = "Copyright (c) Jane Doe 2016. All rights reserved."
category = "Developer Tool"
short_description = "An example application."
long_description = """
Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do
eiusmod tempor incididunt ut labore et dolore magna aliqua.  Ut
enim ad minim veniam, quis nostrud exercitation ullamco laboris
nisi ut aliquip ex ea commodo consequat.
"""
deb_depends = ["libgl1-mesa-glx", "libsdl2-2.0-0 (>= 2.0.5)"]

[package.metadata.bundle.osx]
frameworks = ["SDL2"]
url_schemes = ["com.doe.exampleapplication"]
```

## Contributing

`cargo-bundle` has ambitions to be inclusive project and welcome contributions from anyone.  Please abide by the Rust
code of conduct.

## Status

Very early alpha. Expect the format of the `[package.metadata.bundle]` section to change, and there is no guarantee of
stability.

## License

This program is licensed either under the terms of the
[Apache Software License](http://www.apache.org/licenses/LICENSE-2.0), or the
[MIT License](https://opensource.org/licenses/MIT).
