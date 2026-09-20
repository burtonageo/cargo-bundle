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
        let entitlements = std::fs::read_to_string(entitlements_path).with_context(|| {
            format!("Failed to read Apple signing entitlements {entitlements_path:?}")
        })?;
        signing_settings
            .set_entitlements_xml(SettingsScope::Main, entitlements)
            .map_err(|error| anyhow::anyhow!("Invalid Apple signing entitlements: {error}"))?;
    }
    if settings.apple_signing_hardened_runtime() {
        signing_settings.add_code_signature_flags(SettingsScope::Main, CodeSignatureFlags::RUNTIME);
    }

    UnifiedSigner::new(signing_settings)
        .sign_path_in_place(path)
        .map_err(|error| anyhow::anyhow!("Apple code signing failed: {error}"))
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
            None => Secret::Prompt,
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
    let Some(config) = settings.linux_signing() else {
        return Ok(());
    };

    {
        use sigstore::{bundle::sign::SigningContext, oauth::IdentityToken};

        let token = std::env::var(&config.identity_token_env).with_context(|| {
            format!(
                "Linux Sigstore identity token environment variable `{}` is not set",
                config.identity_token_env
            )
        })?;
        let token = IdentityToken::try_from(token.as_str())
            .map_err(|error| anyhow::anyhow!("Invalid Linux Sigstore identity token: {error}"))?;
        let context = SigningContext::production()
            .map_err(|error| anyhow::anyhow!("Failed to initialize Sigstore: {error}"))?;
        let signer = context.blocking_signer(token).map_err(|error| {
            anyhow::anyhow!("Failed to create Sigstore signing session: {error}")
        })?;

        let signature_paths = artifact_paths
            .iter()
            .map(|artifact_path| sign_linux_artifact(&signer, artifact_path))
            .collect::<crate::Result<Vec<_>>>()?;
        artifact_paths.extend(signature_paths);
        Ok(())
    }
}

fn sign_linux_artifact(
    signer: &sigstore::bundle::sign::blocking::SigningSession<'_>,
    artifact_path: &Path,
) -> crate::Result<std::path::PathBuf> {
    let artifact = std::fs::File::open(artifact_path)
        .with_context(|| format!("Failed to open Linux artifact {artifact_path:?} for signing"))?;
    let bundle = signer
        .sign(artifact)
        .map_err(|error| anyhow::anyhow!("Sigstore signing failed: {error}"))?
        .to_bundle();
    let bundle_path = artifact_path.with_file_name(format!(
        "{}.sigstore.json",
        artifact_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Linux artifact has no file name: {artifact_path:?}"))?
            .to_string_lossy()
    ));
    let bundle_json = serde_json::to_vec_pretty(&bundle)
        .with_context(|| "Failed to serialize Sigstore bundle")?;
    std::fs::write(&bundle_path, bundle_json)
        .with_context(|| format!("Failed to write Sigstore bundle {bundle_path:?}"))?;
    Ok(bundle_path)
}
