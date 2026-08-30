use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Resolve symlinks through the deepest existing ancestor of `path`, then
/// re-append its missing tail.
///
/// Unlike a plain canonicalization fallback, an existing path that cannot be
/// canonicalized, including a dangling symlink, remains an error.
pub fn realpath_missing(path: &Path) -> io::Result<PathBuf> {
    let mut tail = Vec::new();
    let mut current = path;
    loop {
        match dunce::canonicalize(current) {
            Ok(mut base) => {
                base.extend(tail.iter().rev());
                return Ok(base);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::symlink_metadata(current) {
                    Ok(_) => return Err(error),
                    Err(metadata_error) if metadata_error.kind() != io::ErrorKind::NotFound => {
                        return Err(metadata_error);
                    }
                    Err(_) => {}
                }
            }
            Err(error) => return Err(error),
        }
        tail.push(current.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
        })?);
        current = current.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
        })?;
    }
}

#[cfg(test)]
mod tests;
