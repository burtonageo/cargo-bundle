mod category;
mod common;
mod ios_bundle;
mod linux;
mod msi_bundle;
mod osx_bundle;
mod settings;

pub use self::common::{print_error, print_finished};
pub use self::settings::{BuildArtifact, PackageType, Settings};
use crate::bundle::linux::{deb_bundle, rpm_bundle};
use std::path::PathBuf;

pub fn bundle_project(settings: Settings) -> crate::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for package_type in settings.package_types()? {
        paths.append(&mut match package_type {
            PackageType::OsxBundle => osx_bundle::bundle_project(&settings)?,
            PackageType::IosBundle => ios_bundle::bundle_project(&settings)?,
            PackageType::WindowsMsi => msi_bundle::bundle_project(&settings)?,
            PackageType::Deb => deb_bundle::bundle_project(&settings)?,
            PackageType::Rpm => rpm_bundle::bundle_project(&settings)?,
        });
    }
    Ok(paths)
}
