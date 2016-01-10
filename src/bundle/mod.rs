#[cfg(target_os = "macos")]
mod osx_bundle;

#[cfg(target_os = "macos")]
mod native {
    pub use super::osx_bundle::bundle_project;
}

pub use self::native::bundle_project;
