use std::path::{Path, PathBuf};

lazy_static::lazy_static! {
    static ref RESOURCES_ROOT: Option<PathBuf> = _resources_root();
}

fn _resources_root() -> Option<PathBuf> {
    if std::env::var_os("CARGO").is_some() {
        let resources_root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
        let canonical_resources_root = dunce::canonicalize(resources_root).ok()?;
        Some(canonical_resources_root)
    } else {
        // TODO: support for other platforms
        #[cfg(target_os = "macos")]
        {
            let bundle = core_foundation::bundle::CFBundle::main_bundle();
            let bundle_path = bundle.path()?;
            let resources_path = bundle.resources_path()?;
            let absolute_resources_root = bundle_path.join(resources_path);
            let canonical_resources_root = dunce::canonicalize(absolute_resources_root).ok()?;
            Some(canonical_resources_root)
        }
    }
}

/// Returns the absolute path to the resources root, if exists.
///
/// It can handle the difference between `cargo run` and bundled run. When in `cargo run`, it returns what `CARGO_MANIFEST_DIR` environment variable contains. When in bundled run, it varies:
///
/// - On macOS, [CFBundleCopyBundleURL](https://developer.apple.com/documentation/corefoundation/1537142-cfbundlecopybundleurl) and [CFBundleCopyResourcesDirectoryURL](https://developer.apple.com/documentation/corefoundation/1537113-cfbundlecopyresourcesdirectoryur) are used internally. Returned path will be `<bundle-path>/Contents/Resources`, where `<bundle-path>` is the absolute path to the generated `*.app` bundle.
/// - Other platforms are not supported yet,
pub fn resources_root() -> Option<PathBuf> {
    let resources_root = PathBuf::from((*RESOURCES_ROOT).as_ref()?);
    Some(resources_root)
}

/// Returns the absolute form of given path relative to the resources root, if exists.
pub fn resource_path<P: AsRef<Path>>(relative_to_resources_root: P) -> Option<PathBuf> {
    let resources_root = (*RESOURCES_ROOT).as_ref()?;
    let absolute_resource_path = resources_root.join(relative_to_resources_root);
    let canonical_resource_path = dunce::canonicalize(absolute_resource_path).ok()?;
    Some(canonical_resource_path)
}
