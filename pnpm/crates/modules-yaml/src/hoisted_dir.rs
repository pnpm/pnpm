use std::path::{Component, Path, PathBuf};

fn is_safe_part(part: &str) -> bool {
    matches!(Path::new(part).components().next(), Some(Component::Normal(_))) && !part.contains(':')
}

/// A lockfile-relative location resolved against `lockfile_dir`, rebuilt
/// component by component so a location recorded with `/` or `\` gets the
/// platform's separator. `None` for a location that could leave
/// `lockfile_dir`.
#[must_use]
pub fn hoisted_dir(lockfile_dir: &Path, location: &str) -> Option<PathBuf> {
    if location.starts_with('/') || location.starts_with('\\') || Path::new(location).is_absolute()
    {
        return None;
    }
    let mut dir = lockfile_dir.to_path_buf();
    for part in location.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if !is_safe_part(part) {
            return None;
        }
        dir.push(part);
    }
    Some(dir)
}
