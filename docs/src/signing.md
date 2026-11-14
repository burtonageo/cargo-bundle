# Signing

All signing is done in-process. cargo-bundle never starts `codesign`,
`signtool` or `gpg`, so the same configuration works on every host. Keep
certificates, keys and passwords in your CI's secret store, never in version
control, and do not modify an artifact after it is signed.

## Apple

macOS and iOS bundles, and macOS disk images, are signed with
[`apple-codesign`](https://crates.io/crates/apple-codesign) when a certificate
is configured. Export your certificate and private key as a `.p12` file.

```toml
[package.metadata.bundle]
apple_signing_p12 = "secrets/developer-id.p12"
apple_signing_password_env = "APPLE_SIGNING_PASSWORD"
apple_signing_entitlements = "packaging/entitlements.plist"
apple_signing_hardened_runtime = true
apple_signing_timestamp_url = "http://timestamp.apple.com/ts01"
apple_provisioning_profile = "packaging/app.mobileprovision"
```

| Key | Meaning |
| --- | --- |
| `apple_signing_p12` | The certificate. Without it nothing is signed. |
| `apple_signing_password_env` | Name of the environment variable holding its password. Omit it for a passwordless file. |
| `apple_signing_entitlements` | An entitlements plist, XML or binary, whose top level must be a dictionary. |
| `apple_signing_hardened_runtime` | Enable the hardened runtime, which notarization requires. |
| `apple_signing_timestamp_url` | An RFC 3161 timestamp service. |
| `apple_provisioning_profile` | A provisioning profile, copied unchanged to `Contents/embedded.provisionprofile` (macOS) or `embedded.mobileprovision` (iOS). |

The profile is copied in unchanged before signing, so the signature seals it;
cargo-bundle does not inspect it, so check yourself that it has not expired and
matches the `identifier`, the platform and the signing certificate. A disk image
is signed after the app inside it.

### Notarization

```toml
apple_notarization_api_key = "secrets/notary.json"
apple_notarization_timeout = 600
```

The key file is an App Store Connect API key in the format `rcodesign
encode-app-store-connect-api-key` writes. After signing, the app or image is
submitted, cargo-bundle waits up to `apple_notarization_timeout` seconds (600 by
default; zero is rejected) for Apple's verdict, and staples the ticket. Notarization needs a
Developer ID certificate, the hardened runtime and a timestamp; if any is
missing, the run fails before anything is built. The setting
applies to `osx` and `dmg` only; with an iOS format it is an error.

## Windows

Authenticode signing links the GPL-3.0-or-later `osslsigncode`, so it is off by
default. Install with it only if those terms suit the binaries you distribute:

```sh
cargo install cargo-bundle --features windows-signing
```

```toml
[package.metadata.bundle.windows_signing]
certificate_path = "secrets/publisher.p12"
certificate_password_env = "WINDOWS_CERTIFICATE_PASSWORD"
timestamp_url = "http://timestamp.example.com"
```

`timestamp_url` is optional. Without `certificate_password_env` the empty
password is used; the tool never prompts. Configuring signing in a build
without the feature is an error, not a silently unsigned file. The final `.exe`
or `.msi` is signed.

## Linux

Keyless [Sigstore](https://www.sigstore.dev/) signing is applied to `deb`,
`rpm` and `appimage` output when an identity token is available:

```toml
[package.metadata.bundle.linux_signing]
identity_token_env = "SIGSTORE_ID_TOKEN"
```

The variable must hold a fresh OIDC token with the `sigstore` audience, which a
CI provider can mint. Each artifact gets an adjacent `<artifact>.sigstore.json`
bundle with the signature, certificate and transparency-log proof; publish it
alongside.

## Android

Set `signing_key` under `[package.metadata.bundle.android]`; see
[Android](platforms/android.md).
