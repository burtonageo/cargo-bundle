pub(crate) mod appimage;
mod common;
pub(crate) mod deb_bundle;
mod desktop;
mod icons;
pub(crate) mod rpm_bundle;

// Re-export helpers used by deb/appimage packages.
pub(crate) use common::{create_file_with_data, generate_md5sum, tar_and_gzip_dir, total_dir_size};
pub(crate) use desktop::{DesktopFileOptions, generate_desktop_file};
pub(crate) use icons::generate_icon_files;
