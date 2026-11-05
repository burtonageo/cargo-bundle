//! Optional platform code-signing helpers.

use crate::Settings;
use anyhow::Context;
use std::path::Path;

/// Sign an Apple bundle or DMG with the pure-Rust `apple-codesign` library.
pub fn sign_apple_path(settings: &Settings, path: &Path) -> crate::Result<()> {
    use apple_codesign::{
        CodeSignatureFlags, SettingsScope, SigningSettings, UnifiedSigner,
        cryptography::{PrivateKey, parse_pfx_data},
    };

    let Some(p12_path) = settings.apple_signing_p12() else {
        return Ok(());
    };

    let password = match settings.apple_signing_password_env() {
        Some(variable) => std::env::var(variable).with_context(|| {
            format!(
                "Apple signing certificate password environment variable `{variable}` is not set"
            )
        })?,
        // P12 files exported without a password use the empty string.
        None => String::new(),
    };
    let certificate_data = std::fs::read(p12_path)
        .with_context(|| format!("Failed to read Apple signing certificate {p12_path:?}"))?;
    let (certificate, private_key) = parse_pfx_data(&certificate_data, &password)
        .map_err(|error| anyhow::anyhow!("Failed to read Apple signing certificate: {error}"))?;

    let mut signing_settings = SigningSettings::default();
    signing_settings.set_signing_key(private_key.as_key_info_signer(), certificate);
    signing_settings.chain_apple_certificates();
    signing_settings.set_team_id_from_signing_certificate();
    if let Some(timestamp_url) = settings.apple_signing_timestamp_url() {
        signing_settings
            .set_time_stamp_url(timestamp_url)
            .map_err(|error| anyhow::anyhow!("Invalid Apple signing timestamp URL: {error}"))?;
    }
    if let Some(entitlements_path) = settings.apple_signing_entitlements() {
        signing_settings
            .set_entitlements_xml(SettingsScope::Main, entitlements_xml(entitlements_path)?)
            .map_err(|error| anyhow::anyhow!("Invalid Apple signing entitlements: {error}"))?;
    }
    if settings.apple_signing_hardened_runtime() {
        signing_settings.add_code_signature_flags(SettingsScope::Main, CodeSignatureFlags::RUNTIME);
    }

    UnifiedSigner::new(signing_settings)
        .sign_path_in_place(path)
        .map_err(|error| anyhow::anyhow!("Apple code signing failed: {error}"))
}

/// Read an entitlements dictionary stored as an XML or binary plist and
/// return it as the XML that Apple code signatures embed.
fn entitlements_xml(path: &Path) -> crate::Result<String> {
    let entitlements = plist::Value::from_file(path)
        .with_context(|| format!("Failed to read Apple signing entitlements {path:?}"))?;
    anyhow::ensure!(
        entitlements.as_dictionary().is_some(),
        "Apple signing entitlements {path:?} must be a plist dictionary"
    );
    let mut xml = Vec::new();
    entitlements.to_writer_xml(&mut xml)?;
    Ok(String::from_utf8(xml)?)
}

/// Fail before any packaging if notarization is requested but cannot succeed.
/// Apple only accepts hardened-runtime, timestamped, Developer ID-signed code.
pub fn validate_apple_notarization(settings: &Settings) -> crate::Result<()> {
    if settings.apple_notarization_api_key().is_none() {
        return Ok(());
    }
    anyhow::ensure!(
        settings.apple_signing_p12().is_some()
            && settings.apple_signing_hardened_runtime()
            && settings.apple_signing_timestamp_url().is_some(),
        "Apple notarization requires apple_signing_p12, apple_signing_hardened_runtime = true \
         and apple_signing_timestamp_url"
    );
    Ok(())
}

/// Submit a signed app or DMG to Apple, wait for the verdict, and staple the
/// ticket so the result also works offline.
pub fn notarize_apple_path(settings: &Settings, path: &Path) -> crate::Result<()> {
    let Some(api_key) = settings.apple_notarization_api_key() else {
        return Ok(());
    };
    notarize(settings, api_key, path)
}

fn notarize(settings: &Settings, api_key: &Path, path: &Path) -> crate::Result<()> {
    use apple_codesign::{NotarizationUpload, Notarizer, stapling::Stapler};
    use std::time::Duration;

    let notarizer = Notarizer::from_api_key(api_key)
        .map_err(|error| anyhow::anyhow!("Failed to load the notarization API key: {error}"))?;
    let upload = notarizer
        .notarize_path(path, None)
        .map_err(|error| anyhow::anyhow!("Failed to submit {path:?} for notarization: {error}"))?;
    if let NotarizationUpload::UploadId(id) = upload {
        eprintln!("Apple notarization submission ID: {id}");
        let timeout = Duration::from_secs(settings.apple_notarization_timeout());
        let verdict = notarizer
            .wait_on_notarization_and_fetch_log(&id, timeout)
            .map_err(|error| anyhow::anyhow!("Notarization of {path:?} failed: {error}"))?;
        verdict.into_result().map_err(|error| {
            anyhow::anyhow!("Apple rejected {path:?} (submission {id}): {error}")
        })?;
    }
    let stapler = Stapler::new()
        .map_err(|error| anyhow::anyhow!("Failed to set up the ticket stapler: {error}"))?;
    stapler
        .staple_path(path)
        .map_err(|error| anyhow::anyhow!("Failed to staple the ticket to {path:?}: {error}"))?;
    Ok(())
}

/// Copy the configured provisioning profile to `destination` inside a bundle.
/// Call this before signing so the code signature seals the profile.
pub(super) fn embed_apple_provisioning_profile(
    settings: &Settings,
    destination: &Path,
) -> crate::Result<()> {
    let Some(profile) = settings.apple_provisioning_profile() else {
        return Ok(());
    };
    super::common::copy_file(profile, destination)
        .with_context(|| format!("Failed to embed provisioning profile {profile:?}"))
}

/// Sign a Windows executable or installer when configured.
///
/// The implementation is deliberately feature-gated: the vendored
/// osslsigncode implementation is GPL-3.0-or-later.
pub fn sign_windows_artifact(settings: &Settings, artifact_path: &Path) -> crate::Result<()> {
    let Some(config) = settings.windows_signing() else {
        return Ok(());
    };

    #[cfg(not(feature = "windows-signing"))]
    {
        let _ = (artifact_path, config);
        anyhow::bail!(
            "Windows Authenticode signing was requested, but cargo-bundle was built without \
             the `windows-signing` feature. Rebuild it with `--features windows-signing`; \
             that feature links GPL-3.0-or-later code."
        );
    }

    #[cfg(feature = "windows-signing")]
    {
        use osslsigncode::{Credential, Digest, Secret, Timestamp, Unsigned};

        let secret = match &config.certificate_password_env {
            Some(variable) => Secret::value(std::env::var(variable).with_context(|| {
                format!(
                    "Windows signing certificate password environment variable `{variable}` is not set"
                )
            })?),
            // A passwordless P12 uses the empty password. Prompting would hang
            // an unattended build.
            None => Secret::value(String::new()),
        };
        let credential = Credential::pkcs12(&config.certificate_path, secret);
        let parent = artifact_path.parent().ok_or_else(|| {
            anyhow::anyhow!("Windows signing artifact has no parent directory: {artifact_path:?}")
        })?;
        let temporary_directory = tempfile::tempdir_in(parent)
            .with_context(|| "Failed to create temporary directory for Windows signing")?;
        let output_path = temporary_directory.path().join(
            artifact_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Windows signing artifact has no file name"))?,
        );

        let mut signing_job = Unsigned::open(artifact_path)
            .map_err(|error| {
                anyhow::anyhow!("Failed to open Windows artifact for signing: {error}")
            })?
            .sign(credential)
            .digest(Digest::Sha256);
        if let Some(timestamp_url) = &config.timestamp_url {
            signing_job = signing_job.timestamp(Timestamp::rfc3161(timestamp_url));
        }
        signing_job
            .output(&output_path)
            .sign()
            .map_err(|error| anyhow::anyhow!("Windows Authenticode signing failed: {error}"))?;

        std::fs::copy(&output_path, artifact_path).with_context(|| {
            format!(
                "Failed to replace unsigned Windows artifact {artifact_path:?} with its signed copy"
            )
        })?;
        Ok(())
    }
}

/// Creates an adjacent Sigstore bundle for every Linux artifact.
pub fn sign_linux_artifacts(
    settings: &Settings,
    artifact_paths: &mut Vec<std::path::PathBuf>,
) -> crate::Result<()> {
    use sigstore_sign::oidc::IdentityToken;

    let Some(config) = settings.linux_signing() else {
        return Ok(());
    };
    let token = std::env::var(&config.identity_token_env).with_context(|| {
        format!(
            "Linux Sigstore identity token environment variable `{}` is not set",
            config.identity_token_env
        )
    })?;
    let token = IdentityToken::from_jwt(&token)
        .map_err(|error| anyhow::anyhow!("Invalid Linux Sigstore identity token: {error}"))?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to start the Sigstore runtime")?;
    let bundles = runtime.block_on(sign_with_sigstore(token, artifact_paths))?;
    artifact_paths.extend(bundles);
    Ok(())
}

async fn sign_with_sigstore(
    token: sigstore_sign::oidc::IdentityToken,
    artifact_paths: &[std::path::PathBuf],
) -> crate::Result<Vec<std::path::PathBuf>> {
    let context = sigstore_sign::SigningContext::production()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to initialize Sigstore: {error}"))?;
    let signer = context.signer(token);
    let mut bundles = Vec::new();
    for artifact_path in artifact_paths {
        bundles.push(sign_linux_artifact(&signer, artifact_path).await?);
    }
    Ok(bundles)
}

async fn sign_linux_artifact(
    signer: &sigstore_sign::Signer,
    artifact_path: &Path,
) -> crate::Result<std::path::PathBuf> {
    let artifact = std::fs::File::open(artifact_path)
        .with_context(|| format!("Failed to open Linux artifact {artifact_path:?} for signing"))?;
    let bundle = signer
        .sign_reader(artifact)
        .await
        .map_err(|error| anyhow::anyhow!("Sigstore signing failed: {error}"))?;

    let mut bundle_path = artifact_path.as_os_str().to_owned();
    bundle_path.push(".sigstore.json");
    let bundle_path = std::path::PathBuf::from(bundle_path);
    let json = bundle
        .to_json_pretty()
        .context("Failed to serialize Sigstore bundle")?;
    std::fs::write(&bundle_path, json)
        .with_context(|| format!("Failed to write Sigstore bundle {bundle_path:?}"))?;
    Ok(bundle_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary, Value};

    #[test]
    fn entitlements_accept_xml_and_binary_dictionaries_only() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claims.plist");
        let claims = Value::Dictionary(Dictionary::from_iter([
            ("flag", Value::Boolean(true)),
            (
                "groups",
                Value::Array(vec![Value::String("group.app".into())]),
            ),
            ("data", Value::Data(vec![1, 2, 3])),
        ]));
        claims.to_file_xml(&path).unwrap();
        let from_xml = entitlements_xml(&path).unwrap();
        claims.to_file_binary(&path).unwrap();
        assert_eq!(entitlements_xml(&path).unwrap(), from_xml);
        assert_eq!(Value::from_reader_xml(from_xml.as_bytes()).unwrap(), claims);

        Value::Array(vec![]).to_file_binary(&path).unwrap();
        let error = entitlements_xml(&path).unwrap_err().to_string();
        assert!(error.contains("must be a plist dictionary"), "{error}");
        assert!(entitlements_xml(&temp.path().join("missing")).is_err());
    }
}
