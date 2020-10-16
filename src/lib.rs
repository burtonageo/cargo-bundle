use std::path::{Path, PathBuf};

lazy_static::lazy_static! {
    static ref RESOURCES_ROOT: Option<PathBuf> = _resources_root();
}

fn _resources_root() -> Option<PathBuf> {
    if std::env::var_os("CARGO").is_some() {
        let resources_root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
        Some(resources_root)
    } else {
        // TODO: support for other platforms
        #[cfg(target_os = "macos")]
        {
            let bundle = core_foundation::bundle::CFBundle::main_bundle();
            let bundle_path = bundle.path()?;
            let resources_path = bundle.resources_path()?;
            let resources_root = bundle_path.join(resources_path);
            Some(resources_root)
        }
    }
}

/// Returns the absolute path to the resources root, if available. It can handle the difference between `cargo run` and bundled run.
pub fn resources_root() -> Option<PathBuf> {
    let resources_root = PathBuf::from((*RESOURCES_ROOT).as_ref()?);
    Some(resources_root)
}

/// Joins passed path to the resources root.
pub fn resource_path<P: AsRef<Path>>(relative_from_resources_root: P) -> Option<PathBuf> {
    let resources_root = (*RESOURCES_ROOT).as_ref()?;
    let resource_path = resources_root.join(relative_from_resources_root);
    Some(resource_path)
}
