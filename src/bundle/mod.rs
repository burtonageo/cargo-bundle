use {PackageType, Settings};
use std::error::Error;
use std::marker::{Send, Sync};

#[cfg(target_os = "macos")]
mod osx_bundle;
mod deb_bundle;
mod rpm_bundle;

#[cfg(target_os = "macos")]
pub fn bundle_project(settings: Settings) -> Result<(), Box<Error + Send + Sync>> {
    match settings.package_type {
        None | Some(PackageType::OsxBundle) => osx_bundle::bundle_project(&settings),
        Some(PackageType::Deb) => deb_bundle::bundle_project(&settings),
        Some(PackageType::Rpm) => rpm_bundle::bundle_project(&settings),
    }
}

#[cfg(target_os = "windows")]
pub fn bundle_project(settings: Settings) -> Result<(), Box<Error + Send + Sync>> {
    unimplemented!();
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn bundle_project(settings: Settings) -> Result<(), Box<Error + Send + Sync>> {
    match settings.package_type {
        Some(PackageType::Deb) => deb_bundle::bundle_project(&settings),
        Some(PackageType::Rpm) => rpm_bundle::bundle_project(&settings),
        None => deb_bundle::bundle_project(&settings).and_then(|_| rpm_bundle::bundle_project(&settings)),
        Some(otherwise@_) => {
            Err(Box::from(format!("Wrong bundle type {:?}, can only be either `deb` or `rpm`",
                                  otherwise)))
        }
    }
}
