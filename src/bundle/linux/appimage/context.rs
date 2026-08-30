//! Derived paths and architecture for a single AppImage build.

use super::arch::determine_appimage_architecture;
use crate::bundle::Settings;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;

/// Encapsulates all derived paths and configuration for the current build.
pub(super) struct AppImageContext {
    pub(super) package_directory: PathBuf,
    pub(super) package_path: PathBuf,
    pub(super) app_directory_path: PathBuf,
}

impl AppImageContext {
    pub(super) fn new(settings: &Settings) -> crate::Result<Self> {
        let architecture = determine_appimage_architecture(settings.binary_arch())?;
        let package_base_name = format!(
            "{}_{}_{}",
            settings.binary_name(),
            settings.version_string(),
            architecture
        );
        let package_name = format!("{package_base_name}.AppImage");

        let base_directory = settings.project_out_directory().join("bundle/appimage");
        let package_directory = base_directory.join(&package_base_name);
        let package_path = base_directory.join(&package_name);
        let app_directory_path = package_directory.join("AppDir");

        Ok(Self {
            package_directory,
            package_path,
            app_directory_path,
        })
    }

    pub(super) fn clean_previous_build(&self) -> crate::Result<()> {
        if self.package_directory.exists() {
            fs::remove_dir_all(&self.package_directory)
                .with_context(|| "Failed to remove old package directory")?;
        }
        if self.package_path.exists() {
            let _ = fs::remove_file(&self.package_path);
        }
        Ok(())
    }
}
