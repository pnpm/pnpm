use std::{
    fs, io,
    path::{Component, Path},
};

pub(super) fn validate_relative_path(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty()
        || path.components().any(|part| !matches!(part, Component::Normal(_)))
        || path.components().any(|part| {
            part.as_os_str().eq_ignore_ascii_case(".git")
                || part.as_os_str().eq_ignore_ascii_case("node_modules")
        })
    {
        return Err(io::Error::other(format!(
            "Cache path must stay inside the project: {}",
            path.display(),
        )));
    }
    Ok(())
}

pub(super) fn check_ancestors(root: &Path, relative: &Path) -> io::Result<()> {
    reject_symlink(root)?;
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        reject_symlink(&path)?;
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::other(format!("Cache path is a symlink: {}", path.display())));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}
