use std::{
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Resolve existing symlinked components left-to-right before applying `..`,
/// keeping any non-existent tail components normalized lexically.
pub fn resolve_path_with_symlinks(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot resolve an empty path"));
    }
    let (mut acc, components) = seed_absolute_anchor(path)?;
    let mut in_missing_tail = false;
    for component in components {
        apply_path_component(component, &mut acc, &mut in_missing_tail);
    }
    Ok(acc)
}

fn seed_absolute_anchor(path: &Path) -> io::Result<(PathBuf, std::path::Components<'_>)> {
    let mut components = path.components();
    if path.is_absolute() {
        return Ok((PathBuf::new(), components));
    }
    match components.clone().next() {
        Some(Component::Prefix(prefix)) => seed_prefix_anchor(prefix.as_os_str(), components),
        Some(Component::RootDir) => {
            components.next();
            Ok((std::path::absolute(Component::RootDir.as_os_str())?, components))
        }
        _ => Ok((std::env::current_dir()?, components)),
    }
}

fn seed_prefix_anchor<'a>(
    prefix: &OsStr,
    mut components: std::path::Components<'a>,
) -> io::Result<(PathBuf, std::path::Components<'a>)> {
    components.next();
    let mut anchor = PathBuf::from(prefix);
    if matches!(components.clone().next(), Some(Component::RootDir)) {
        components.next();
        anchor.push(Component::RootDir.as_os_str());
    }
    Ok((std::path::absolute(anchor)?, components))
}

fn apply_path_component(component: Component<'_>, acc: &mut PathBuf, in_missing_tail: &mut bool) {
    match component {
        Component::Prefix(_) | Component::RootDir => acc.push(component.as_os_str()),
        Component::CurDir => {}
        Component::Normal(part) => push_normal_component(part, acc, in_missing_tail),
        Component::ParentDir => pop_parent_component(acc, in_missing_tail),
    }
}

fn push_normal_component(part: &OsStr, acc: &mut PathBuf, in_missing_tail: &mut bool) {
    acc.push(part);
    if *in_missing_tail {
        return;
    }
    match dunce::canonicalize(&*acc) {
        Ok(canon) => *acc = canon,
        Err(err)
            if matches!(err.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) =>
        {
            *in_missing_tail = true;
        }
        Err(_) => {}
    }
}

fn pop_parent_component(acc: &mut PathBuf, in_missing_tail: &mut bool) {
    if !*in_missing_tail && let Ok(canon) = dunce::canonicalize(&*acc) {
        *acc = canon;
    }
    acc.pop();
    if *in_missing_tail && let Ok(canon) = dunce::canonicalize(&*acc) {
        *acc = canon;
        *in_missing_tail = false;
    }
}

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

/// The real path of `dir` when a symlink lies between `base` and `dir`,
/// resolved through the deepest existing ancestor as [`realpath_missing`] does.
/// When `dir` is not under `base`, a symlink anywhere on its path counts.
///
/// `None` when every component of `dir` below `base` is a real directory (or
/// does not exist yet), or when either path cannot be resolved. Relative
/// symlinks written into `dir` must be computed from the returned path, since
/// the OS resolves them from there.
#[must_use]
pub fn symlinked_dir_real_path(base: &Path, dir: &Path) -> Option<PathBuf> {
    let real = realpath_missing(dir).ok()?;
    let lexical_real = match dir.strip_prefix(base) {
        Ok(below_base) => dunce::canonicalize(base).ok()?.join(below_base),
        Err(_) => dir.to_path_buf(),
    };
    (real != crate::lexical_normalize(&lexical_real)).then_some(real)
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
