// An APK is assembled, aligned and signed in pure Rust by the `apk` crate. The
// input must be a NativeActivity shared library named `lib<name>.so` (a Rust
// `cdylib`) given with `--binary-path`; resources become `assets/` and the largest PNG icon becomes the
// launcher icon.

use super::common;
use crate::Settings;
use anyhow::Context;
use apk::{
    Abi, ApiLevel, ApkBuilder, Manifest, VersionCode,
    res::Density,
    sign::{Signer, SigningIdentity},
};
use std::{fs, path::PathBuf};

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let abi = match settings.binary_arch() {
        "aarch64" => Abi::Arm64V8a,
        "arm" => Abi::ArmeabiV7a,
        "x86" => Abi::X86,
        "x86_64" => Abi::X86_64,
        arch => anyhow::bail!("Unsupported Android architecture {arch:?}"),
    };
    let library = settings
        .binary_path()
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("lib")?.strip_suffix(".so"))
        .context("Android bundling needs a cdylib: pass --binary-path <dir>/lib<name>.so")?;
    let android = settings.android();

    let project = settings
        .manifest_path()
        .parent()
        .context("The package manifest has no directory")?;
    let mut apk = match &android.manifest {
        Some(path) => {
            let path = project.join(path);
            let xml = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read android.manifest {path:?}"))?;
            ApkBuilder::new(Manifest::parse(&xml).context("Invalid android.manifest")?)
        }
        None => {
            let identifier = settings.bundle_identifier();
            ApkBuilder::native_activity(
                identifier
                    .parse()
                    .with_context(|| format!("Invalid Android package name {identifier:?}"))?,
                library.parse()?,
            )
        }
    };
    let version_code = android.version_code.unwrap_or(1);
    apk.label(settings.bundle_name()).version(
        VersionCode::try_from(version_code)
            .with_context(|| format!("Invalid android.version_code {version_code}"))?,
        &settings.version_string().to_string(),
    );
    if let Some(level) = android.min_sdk {
        let level = ApiLevel::try_from(level)
            .with_context(|| format!("Invalid android.min_sdk {level}"))?;
        apk.min_sdk(level);
    }
    if let Some(level) = android.target_sdk {
        let level = ApiLevel::try_from(level)
            .with_context(|| format!("Invalid android.target_sdk {level}"))?;
        apk.target_sdk(level);
    }
    for permission in &android.permissions {
        apk.permission(
            permission
                .parse()
                .with_context(|| format!("Invalid Android permission {permission:?}"))?,
        );
    }
    apk.native_library_bytes(abi, library.parse()?, settings.binary_path())?;
    for resource in settings.resource_files() {
        let resource = resource?;
        apk.asset(
            &common::archive_path(&common::resource_relpath(&resource))?,
            resource,
        )?;
    }
    let icons = settings.icon_files().collect::<crate::Result<Vec<_>>>()?;
    let largest_png = icons
        .into_iter()
        .filter(|icon| icon.extension().is_some_and(|extension| extension == "png"))
        .max_by_key(|icon| image::image_dimensions(icon).map_or(0, |(width, _)| width));
    if let Some(icon) = largest_png {
        apk.icon(Density::XXXHDPI, icon)?;
    }
    match &android.signing_key {
        Some(key) => {
            let key = project.join(key);
            let identity = SigningIdentity::from_pem_file(&key)
                .with_context(|| format!("Failed to load Android signing key {key:?}"))?;
            apk.sign_with(Signer::new(identity));
        }
        None => common::print_warning(
            "The APK is unsigned and cannot be installed; set android.signing_key.",
        )?,
    }

    let directory = settings.project_out_directory().join("bundle/apk");
    fs::create_dir_all(&directory)
        .with_context(|| format!("Failed to create bundle directory {directory:?}"))?;
    let file_name = format!("{}.apk", settings.bundle_name());
    common::print_bundling(&file_name)?;
    let output = directory.join(file_name);
    apk.write(&output).context("Failed to assemble the APK")?;
    Ok(vec![output])
}
