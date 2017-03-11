use {PackageType, Settings};
use std::path::PathBuf;

mod common;
mod deb_bundle;
mod ios_bundle;
mod osx_bundle;
mod rpm_bundle;

pub fn bundle_project(settings: Settings) -> ::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for package_type in settings.package_types()? {
        paths.append(&mut match package_type {
            PackageType::OsxBundle => osx_bundle::bundle_project(&settings)?,
            PackageType::IosBundle => ios_bundle::bundle_project(&settings)?,
            PackageType::Deb => deb_bundle::bundle_project(&settings)?,
            PackageType::Rpm => rpm_bundle::bundle_project(&settings)?,
        });
    }
    Ok(paths)
}
