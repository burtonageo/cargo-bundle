//! Shared Linux packaging utilities (archives, checksums, small file helpers).

use crate::bundle::common;
use libflate::gzip;
use md5::Digest;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Creates a `.tar.gz` file from the given directory (placing the new file
/// within the given directory's parent directory), then deletes the original
/// directory and returns the path to the new file.
pub fn tar_and_gzip_dir<P: AsRef<Path>>(src_dir: P) -> crate::Result<PathBuf> {
    let src_dir = src_dir.as_ref();
    let dest_path = src_dir.with_extension("tar.gz");
    let dest_file = common::create_file(&dest_path)?;
    let gzip_encoder = gzip::Encoder::new(dest_file)?;
    let gzip_encoder = create_tar_from_dir(src_dir, gzip_encoder)?;
    let mut dest_file = gzip_encoder.finish().into_result()?;
    dest_file.flush()?;
    Ok(dest_path)
}

/// Writes a tar file to the given writer containing the given directory.
pub fn create_tar_from_dir<P: AsRef<Path>, W: Write>(src_dir: P, dest_file: W) -> crate::Result<W> {
    let src_dir = src_dir.as_ref();
    let mut tar_builder = tar::Builder::new(dest_file);
    for entry in WalkDir::new(src_dir) {
        let entry = entry?;
        let src_path = entry.path();
        if src_path == src_dir {
            continue;
        }
        let dest_path = src_path.strip_prefix(src_dir).unwrap();
        if entry.file_type().is_dir() {
            tar_builder.append_dir(dest_path, src_path)?;
        } else {
            let mut src_file = File::open(src_path)?;
            tar_builder.append_file(dest_path, &mut src_file)?;
        }
    }
    let dest_file = tar_builder.into_inner()?;
    Ok(dest_file)
}

/// Create an empty file at the given path, creating any parent directories as
/// needed, then write `data` into the file.
pub fn create_file_with_data<P: AsRef<Path>>(path: P, data: &str) -> crate::Result<()> {
    let mut file = common::create_file(path.as_ref())?;
    file.write_all(data.as_bytes())?;
    file.flush()?;
    Ok(())
}

/// Computes the total size, in bytes, of the given directory and all of its
/// contents.
pub fn total_dir_size(dir: &Path) -> crate::Result<u64> {
    let mut total: u64 = 0;
    for entry in WalkDir::new(dir) {
        total += entry?.metadata()?.len();
    }
    Ok(total)
}

/// Compute the md5 hash of the given file.
pub fn generate_md5sum(file_path: &Path) -> crate::Result<Digest> {
    let mut file = File::open(file_path)?;
    let mut hash = md5::Context::new();
    io::copy(&mut file, &mut hash)?;
    Ok(hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_tar_and_gzip_dir() {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir(temp_dir.path().join("foo")).unwrap();
        File::create(temp_dir.path().join("foo/file1.txt")).unwrap();
        std::fs::create_dir_all(temp_dir.path().join("foo/subdir")).unwrap();
        File::create(temp_dir.path().join("foo/subdir/file2.txt"))
            .unwrap()
            .write_all(b"test")
            .unwrap();
        let tar_gz_file = tar_and_gzip_dir(temp_dir.path().join("foo"));
        assert!(tar_gz_file.is_ok());
        let tar_gz_file = tar_gz_file.unwrap();

        assert!(tar_gz_file.exists());
        assert!(tar_gz_file.metadata().unwrap().len() > 0);
    }

    #[test]
    fn test_create_file_with_data() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("foo.txt");
        assert!(create_file_with_data(&file_path, "test").is_ok());
        assert!(file_path.exists());
        assert_eq!(file_path.metadata().unwrap().len(), 4);
    }

    #[test]
    fn test_generate_md5sum() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("foo.txt");
        File::create(&file_path)
            .unwrap()
            .write_all(b"test")
            .unwrap();
        let md5_sums = generate_md5sum(file_path.as_path());
        assert!(md5_sums.is_ok());
        let mut md5_str = String::new();

        for b in md5_sums.unwrap().iter() {
            md5_str.push_str(&format!("{b:02x}"));
        }

        assert_eq!(md5_str, "098f6bcd4621d373cade4e832627b4f6".to_string());
    }
}
