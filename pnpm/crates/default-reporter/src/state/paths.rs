use super::{Component, Path, PathBuf, normalize};

pub(super) fn normalized_prefix(cwd: &str, prefix: &str) -> String {
    let cwd = normalize(cwd);
    let prefix = normalize(prefix);
    let path = Path::new(&prefix);
    let absolute = if path.is_absolute() { path.to_path_buf() } else { Path::new(&cwd).join(path) };
    let normalized = normalize(&lexically_normalize(&absolute).to_string_lossy());
    strip_trailing_separators(&normalized)
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn strip_trailing_separators(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() && path.starts_with('/') { "/".to_string() } else { trimmed.to_string() }
}
