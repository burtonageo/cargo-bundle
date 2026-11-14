use crate::Settings;
use std::path::PathBuf;

pub fn bundle_project(_settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    anyhow::bail!("RPM packages are not implemented yet; use `--format deb` or `--format appimage`")
}
