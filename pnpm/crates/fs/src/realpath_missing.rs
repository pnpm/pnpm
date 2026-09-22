use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
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
        if let Some(mut base) = canonicalize_existing(current)? {
            base.extend(tail.iter().rev());
            return Ok(base);
        }
        tail.push(
            current
                .file_name()
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
                })?,
        );
        current = current
            .parent()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
            })?;
    }
}

fn canonicalize_existing(path: &Path) -> io::Result<Option<PathBuf>> {
    for _ in 0..2 {
        match dunce::canonicalize(path) {
            Ok(path) => return Ok(Some(path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::symlink_metadata(path) {
                    Ok(_) => continue,
                    Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => {
                        return Ok(None);
                    }
                    Err(metadata_error) => return Err(metadata_error),
                }
            }
            Err(error) => return Err(error),
        }
    }
    dunce::canonicalize(path).map(Some)
}

#[cfg(test)]
mod tests;
