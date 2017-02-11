use {PackageType, Settings};
use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod osx_bundle;
mod deb_bundle;
mod rpm_bundle;

#[cfg(target_os = "macos")]
pub fn bundle_project(settings: Settings) -> ::Result<Vec<PathBuf>> {
    match settings.package_type {
        None |
        Some(PackageType::OsxBundle) => osx_bundle::bundle_project(&settings),
        Some(PackageType::Deb) => deb_bundle::bundle_project(&settings),
        Some(PackageType::Rpm) => rpm_bundle::bundle_project(&settings),
    }
}

#[cfg(target_os = "windows")]
pub fn bundle_project(settings: Settings) -> ::Result<Vec<PathBuf>> {
    match settings.package_type {
        None => Err(Box::from("Native windows bundles not yet supported")),
        Some(PackageType::Deb) => deb_bundle::bundle_project(&settings),
        Some(PackageType::Rpm) => rpm_bundle::bundle_project(&settings),
        Some(otherwise) => {
            bail!("Wrong bundle type {:?}, can only be either `deb`, `rpm` or `win`",
                  otherwise)
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn bundle_project(settings: Settings) -> ::Result<Vec<PathBuf>> {
    match settings.package_type {
        None => {
            deb_bundle::bundle_project(&settings).and_then(|mut deb_path| {
                let mut rpm_path = rpm_bundle::bundle_project(&settings)?;
                deb_path.append(&mut rpm_path);
                Ok(deb_path)
            })
        }
        Some(PackageType::Deb) => deb_bundle::bundle_project(&settings),
        Some(PackageType::Rpm) => rpm_bundle::bundle_project(&settings),
        Some(otherwise) => {
            bail!("Wrong bundle type {:?}, can only be either `deb` or `rpm`",
                  otherwise)
        }
    }
}
