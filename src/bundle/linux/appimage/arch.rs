//! Cargo / host architecture → AppImage runtime architecture names.

/// Map a Cargo/`std` arch name to the AppImage type-2 runtime arch suffix.
pub fn determine_appimage_architecture(cargo_architecture: &str) -> crate::Result<&'static str> {
    match cargo_architecture {
        "x86_64" => Ok("x86_64"),
        "aarch64" | "arm64" => Ok("aarch64"),
        "i686" | "x86" | "i586" | "i386" => Ok("i686"),
        "armhf" | "arm" | "armv7" | "armv7l" => Ok("armhf"),
        other => anyhow::bail!("Unsupported architecture for AppImage bundling: `{other}`."),
    }
}
