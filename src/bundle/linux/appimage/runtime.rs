//! Local type-2 AppImage runtime loading and validation.

use crate::bundle::Settings;
use anyhow::Context;
use std::fs;

/// ELF magic used to sanity-check the supplied runtime.
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";

pub(super) fn read(settings: &Settings) -> crate::Result<Vec<u8>> {
    let path = settings.appimage_runtime_path().ok_or_else(|| {
        anyhow::anyhow!(
            "appimage_runtime_path is required: cargo-bundle does not download AppImage runtimes"
        )
    })?;
    let data =
        fs::read(path).with_context(|| format!("Failed to read appimage_runtime_path {path:?}"))?;
    validate_bytes(&data, &path.display().to_string())?;
    Ok(data)
}

fn validate_bytes(data: &[u8], source: &str) -> crate::Result<()> {
    if data.len() < 4 || &data[..4] != ELF_MAGIC {
        anyhow::bail!("AppImage runtime from {source} lacks ELF magic");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_runtime_rejects_garbage() {
        assert!(validate_bytes(b"not an elf", "test").is_err());
    }

    #[test]
    fn validate_runtime_accepts_fake_elf() {
        assert!(validate_bytes(ELF_MAGIC, "fake").is_ok());
    }
}
