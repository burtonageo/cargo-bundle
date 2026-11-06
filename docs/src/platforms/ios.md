# iOS

iOS support is experimental. `ios` builds `<Name>.app`; `ipa` wraps that app in
`Payload/` as a ZIP archive that can be installed on a device. The archive is
deterministic: entries are sorted with fixed timestamps and permissions, so the
same inputs give the same bytes.

```sh
cargo bundle --format ipa --target aarch64-apple-ios \
    --binary-path target/aarch64-apple-ios/release/my-app
```

`ipa` requires exactly `--target aarch64-apple-ios`. Simulator, Mac Catalyst and
macOS targets cannot be installed from an IPA and are refused.

Links inside the app, such as a framework's `Versions/Current`, are archived
as links; one that points outside the app is an error. Notarization is for
macOS only, so `apple_notarization_api_key` with an iOS format is refused.

## Requirements

- `identifier` must be set, made of dot-separated parts of letters, digits and
  hyphens. An empty identifier produces an app a device will not install.
- To run on a device the app must be signed with a provisioning profile; see
  [Signing](../signing.md#apple).

## What goes in the app

The app contains your executable (mode `0755`), a generated `Info.plist`,
[resources](../resources.md) at its root, and icons. An SVG icon is rendered at
every size iPhone and iPad ask for (20px to 1024px) and listed in
`CFBundleIconFiles`; without one, PNG and ICNS inputs are used as they are.
Localized strings go in `<locale>.lproj/InfoPlist.strings`:

```toml
[package.metadata.bundle.ios.localizations.fr]
CFBundleDisplayName = "Mon Application"
```

Everything is written before signing, so the signature seals it.
