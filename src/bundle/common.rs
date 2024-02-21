use ::ResultExt;
use std;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Component, Path, PathBuf};
use term;
use walkdir;

/// Returns true if the path has a filename indicating that it is a high-desity
/// "retina" icon.  Specifically, returns true the the file stem ends with
/// "@2x" (a convention specified by the [Apple developer docs](
/// https://developer.apple.com/library/mac/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Optimizing/Optimizing.html)).
pub fn is_retina<P: AsRef<Path>>(path: P) -> bool {
    path.as_ref()
        .file_stem()
        .and_then(OsStr::to_str)
        .map(|stem| stem.ends_with("@2x"))
        .unwrap_or(false)
}

/// Creates a new file at the given path, creating any parent directories as
/// needed.
pub fn create_file(path: &Path) -> ::Result<BufWriter<File>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(&parent).chain_err(|| {
            format!("Failed to create directory {:?}", parent)
        })?;
    }
    let file = File::create(path).chain_err(|| {
        format!("Failed to create file {:?}", path)
    })?;
    Ok(BufWriter::new(file))
}

#[cfg(unix)]
fn symlink_dir(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink_dir(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
}

#[cfg(unix)]
fn symlink_file(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink_file(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

/// Copies a regular file from one path to another, creating any parent
/// directories of the destination path as necessary.  Fails if the source path
/// is a directory or doesn't exist.
pub fn copy_file(from: &Path, to: &Path) -> ::Result<()> {
    if !from.exists() {
        bail!("{:?} does not exist", from);
    }
    if !from.is_file() {
        bail!("{:?} is not a file", from);
    }
    let dest_dir = to.parent().unwrap();
    fs::create_dir_all(dest_dir).chain_err(|| {
        format!("Failed to create {:?}", dest_dir)
    })?;
    fs::copy(from, to).chain_err(|| {
        format!("Failed to copy {:?} to {:?}", from, to)
    })?;
    Ok(())
}

/// Recursively copies a directory file from one path to another, creating any
/// parent directories of the destination path as necessary.  Fails if the
/// source path is not a directory or doesn't exist, or if the destination path
/// already exists.
pub fn copy_dir(from: &Path, to: &Path) -> ::Result<()> {
    if !from.exists() {
        bail!("{:?} does not exist", from);
    }
    if !from.is_dir() {
        bail!("{:?} is not a directory", from);
    }
    if to.exists() {
        bail!("{:?} already exists", to);
    }
    let parent = to.parent().unwrap();
    fs::create_dir_all(parent).chain_err(|| {
        format!("Failed to create {:?}", parent)
    })?;
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry?;
        debug_assert!(entry.path().starts_with(from));
        let rel_path = entry.path().strip_prefix(from).unwrap();
        let dest_path = to.join(rel_path);
        if entry.file_type().is_symlink() {
            let target = fs::read_link(entry.path())?;
            if entry.path().is_dir() {
                symlink_dir(&target, &dest_path)?;
            } else {
                symlink_file(&target, &dest_path)?;
            }
        } else if entry.file_type().is_dir() {
            fs::create_dir(dest_path)?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

/// Given a path (absolute or relative) to a resource file, returns the
/// relative path from the bundle resources directory where that resource
/// should be stored.
pub fn resource_relpath(path: &Path) -> PathBuf {
    let mut dest = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => {}
            Component::RootDir => dest.push("_root_"),
            Component::CurDir => {}
            Component::ParentDir => dest.push("_up_"),
            Component::Normal(string) => dest.push(string),
        }
    }
    dest
}

/// Prints a message to stderr, in the same format that `cargo` uses,
/// indicating that we are creating a bundle with the given filename.
pub fn print_bundling(filename: &str) -> ::Result<()> {
    print_progress("Bundling", filename)
}

/// Prints a message to stderr, in the same format that `cargo` uses,
/// indicating that we have finished the the given bundles.
pub fn print_finished(output_paths: &Vec<PathBuf>) -> ::Result<()> {
    let pluralised = if output_paths.len() == 1 {
        "bundle"
    } else {
        "bundles"
    };
    let msg = format!("{} {} at:", output_paths.len(), pluralised);
    print_progress("Finished", &msg)?;
    for path in output_paths {
        println!("        {}", path.display());
    }
    Ok(())
}

fn safe_term_attr<T : term::Terminal + ?Sized>(output: &mut Box<T>, attr: term::Attr) -> term::Result<()> {
    match output.supports_attr(attr) {
        true => output.attr(attr),
        false => Ok(()),
    }
}

fn print_progress(step: &str, msg: &str) -> ::Result<()> {
    if let Some(mut output) = term::stderr() {
        safe_term_attr(&mut output, term::Attr::Bold)?;
        output.fg(term::color::GREEN)?;
        write!(output, "    {}", step)?;
        output.reset()?;
        write!(output, " {}\n", msg)?;
        output.flush()?;
        Ok(())
    } else {
        let mut output = io::stderr();
        write!(output, "    {}", step)?;
        write!(output, " {}\n", msg)?;
        output.flush()?;
        Ok(())
    }
}

/// Prints a warning message to stderr, in the same format that `cargo` uses.
pub fn print_warning(message: &str) -> ::Result<()> {
    if let Some(mut output) = term::stderr() {
        safe_term_attr(&mut output, term::Attr::Bold)?;
        output.fg(term::color::YELLOW)?;
        write!(output, "warning:")?;
        output.reset()?;
        write!(output, " {}\n", message)?;
        output.flush()?;
        Ok(())
    } else {
        let mut output = io::stderr();
        write!(output, "warning:")?;
        write!(output, " {}\n", message)?;
        output.flush()?;
        Ok(())
    }
}

/// Prints an error to stderr, in the same format that `cargo` uses.
pub fn print_error(error: &::Error) -> ::Result<()> {
    if let Some(mut output) = term::stderr() {
        safe_term_attr(&mut output, term::Attr::Bold)?;
        output.fg(term::color::RED)?;
        write!(output, "error:")?;
        output.reset()?;
        safe_term_attr(&mut output, term::Attr::Bold)?;
        writeln!(output, " {}", error)?;
        output.reset()?;
        for cause in error.iter().skip(1) {
            writeln!(output, "  Caused by: {}", cause)?;
        }
        if let Some(backtrace) = error.backtrace() {
            writeln!(output, "{:?}", backtrace)?;
        }
        output.flush()?;
        Ok(())
    } else {
        let mut output = io::stderr();
        write!(output, "error:")?;
        writeln!(output, " {}", error)?;
        for cause in error.iter().skip(1) {
            writeln!(output, "  Caused by: {}", cause)?;
        }
        if let Some(backtrace) = error.backtrace() {
            writeln!(output, "{:?}", backtrace)?;
        }
        output.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std;
    use std::io::Write;
    use std::path::PathBuf;
    use super::{copy_dir, create_file, symlink_file, is_retina, resource_relpath};
    use tempfile;

    #[test]
    fn create_file_with_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!tmp.path().join("parent").exists());
        {
            let mut file = create_file(&tmp.path().join("parent/file.txt")).unwrap();
            write!(file, "Hello, world!\n").unwrap();
        }
        assert!(tmp.path().join("parent").is_dir());
        assert!(tmp.path().join("parent/file.txt").is_file());
    }

    #[test]
    fn copy_dir_with_symlinks() {
        // Create a directory structure that looks like this:
        //   ${TMP}/orig/
        //       sub/
        //           file.txt
        //       link -> sub/file.txt
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut file = create_file(&tmp.path().join("orig/sub/file.txt")).unwrap();
            write!(file, "Hello, world!\n").unwrap();
        }
        symlink_file(&PathBuf::from("sub/file.txt"),
                     &tmp.path().join("orig/link")).unwrap();
        assert_eq!(std::fs::read(tmp.path().join("orig/link")).unwrap().as_slice(),
                   b"Hello, world!\n");
        // Copy ${TMP}/orig to ${TMP}/parent/copy, and make sure that the
        // directory structure, file, and symlink got copied correctly.
        copy_dir(&tmp.path().join("orig"), &tmp.path().join("parent/copy")).unwrap();
        assert!(tmp.path().join("parent/copy").is_dir());
        assert!(tmp.path().join("parent/copy/sub").is_dir());
        assert!(tmp.path().join("parent/copy/sub/file.txt").is_file());
        assert_eq!(std::fs::read(tmp.path().join("parent/copy/sub/file.txt")).unwrap().as_slice(),
                   b"Hello, world!\n");
        assert!(tmp.path().join("parent/copy/link").exists());
        assert_eq!(std::fs::read_link(tmp.path().join("parent/copy/link")).unwrap(),
                   PathBuf::from("sub/file.txt"));
        assert_eq!(std::fs::read(tmp.path().join("parent/copy/link")).unwrap().as_slice(),
                   b"Hello, world!\n");
    }

    #[test]
    fn retina_icon_paths() {
        assert!(!is_retina("data/icons/512x512.png"));
        assert!(is_retina("data/icons/512x512@2x.png"));
    }

    #[test]
    fn resource_relative_paths() {
        assert_eq!(resource_relpath(&PathBuf::from("./data/images/button.png")),
                   PathBuf::from("data/images/button.png"));
        assert_eq!(resource_relpath(&PathBuf::from("../../images/wheel.png")),
                   PathBuf::from("_up_/_up_/images/wheel.png"));
        assert_eq!(resource_relpath(&PathBuf::from("/home/ferris/crab.png")),
                   PathBuf::from("_root_/home/ferris/crab.png"));
    }
}
