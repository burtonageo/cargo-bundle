mod category;
mod common;
mod dmg_bundle;
#[cfg(unix)]
mod dmg_finder;
mod ios_bundle;
mod linux;
mod localization;
mod msi_bundle;
mod osx_bundle;
mod settings;
mod signing;
mod windows;
mod wxsmsi_bundle;

pub use self::common::{print_error, print_finished};
pub use self::settings::{BuildArtifact, PackageType, Settings};
use crate::bundle::linux::{appimage, deb_bundle, rpm_bundle};
use std::path::PathBuf;

pub fn bundle_project(settings: Settings) -> crate::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for package_type in settings.package_types()? {
        let mut package_paths = match package_type {
            PackageType::OsxBundle => osx_bundle::bundle_project(&settings)?,
            PackageType::OsxDmg => dmg_bundle::bundle_project(&settings)?,
            PackageType::IosBundle => ios_bundle::bundle_project(&settings)?,
            PackageType::WindowsMsi => msi_bundle::bundle_project(&settings)?,
            PackageType::WxsMsi => wxsmsi_bundle::bundle_project(&settings)?,
            PackageType::WindowsBundle => windows::exe_bundle::bundle_project(&settings)?,
            PackageType::Deb => deb_bundle::bundle_project(&settings)?,
            PackageType::Rpm => rpm_bundle::bundle_project(&settings)?,
            PackageType::AppImage => appimage::bundle_project(&settings)?,
        };
        if matches!(
            package_type,
            PackageType::Deb | PackageType::Rpm | PackageType::AppImage
        ) {
            signing::sign_linux_artifacts(&settings, &mut package_paths)?;
        }
        paths.append(&mut package_paths);
    }
    Ok(paths)
}
