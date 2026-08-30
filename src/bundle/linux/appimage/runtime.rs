//! Type-2 AppImage runtime fetching, validation, and disk caching.

use crate::bundle::common;
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};

/// Official continuous type-2 runtime release base URL.
const DEFAULT_RUNTIME_URL_BASE: &str =
    "https://github.com/AppImage/type2-runtime/releases/download/continuous";

/// ELF magic used to sanity-check downloaded runtimes.
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";

/// Minimum plausible runtime size (bytes). Real type2 runtimes are ~700KB+.
const MINIMUM_RUNTIME_SIZE: u64 = 64 * 1024;

pub(super) fn read(architecture: &str) -> crate::Result<Vec<u8>> {
    let cache_path = cache_path(architecture)?;
    if let Ok(data) = fs::read(&cache_path)
        && validate_bytes(&data, &cache_path.display().to_string()).is_ok()
    {
        return Ok(data);
    }
    let _ = fs::remove_file(&cache_path);

    let url = format!("{DEFAULT_RUNTIME_URL_BASE}/runtime-{architecture}");
    let response = reqwest::blocking::get(&url)
        .with_context(|| format!("Failed to download AppImage type-2 runtime from {url}"))?;
    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to download runtime from {url}: HTTP {}. Check your network connection.",
            response.status()
        );
    }
    let data = response
        .bytes()
        .with_context(|| format!("Failed to read runtime response from {url}"))?
        .to_vec();

    validate_bytes(&data, &url)?;
    cache_bytes(&cache_path, &data);
    Ok(data)
}

fn validate_bytes(data: &[u8], source: &str) -> crate::Result<()> {
    if (data.len() as u64) < MINIMUM_RUNTIME_SIZE {
        anyhow::bail!(
            "AppImage runtime from {source} is too small ({} bytes)",
            data.len()
        );
    }
    if data.len() < 4 || &data[..4] != ELF_MAGIC {
        anyhow::bail!("AppImage runtime from {source} lacks ELF magic");
    }
    Ok(())
}

fn cache_path(architecture: &str) -> crate::Result<PathBuf> {
    let cache_root = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."));
    Ok(cache_root
        .join("cargo-bundle")
        .join("appimage-runtime")
        .join(architecture))
}

fn cache_bytes(cache_path: &Path, data: &[u8]) {
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::write(cache_path, data).is_err() {
        let _ = common::print_warning(&format!(
            "Could not cache AppImage runtime at {}",
            cache_path.display()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_runtime_rejects_garbage() {
        assert!(validate_bytes(b"not an elf", "test").is_err());
        assert!(validate_bytes(&[0x7f, b'E', b'L', b'F'], "tiny").is_err());
    }

    #[test]
    fn validate_runtime_accepts_fake_elf() {
        let mut data = vec![0u8; MINIMUM_RUNTIME_SIZE as usize];
        data[0..4].copy_from_slice(ELF_MAGIC);
        assert!(validate_bytes(&data, "fake").is_ok());
    }

    #[test]
    fn runtime_cache_path_contains_arch() {
        let path = cache_path("x86_64").unwrap();
        assert!(path.ends_with("x86_64"));
        assert!(path.to_string_lossy().contains("appimage-runtime"));
    }

    #[test]
    fn default_runtime_url_format() {
        let arch = "aarch64";
        let url = format!("{DEFAULT_RUNTIME_URL_BASE}/runtime-{arch}");
        assert_eq!(
            url,
            "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-aarch64"
        );
    }
}
