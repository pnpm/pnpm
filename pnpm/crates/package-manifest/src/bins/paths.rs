/// Normalize a package-relative path using `/` separators, rejecting paths
/// that start with `/`, contain `\` or `:`, or climb above the package root
/// via `..`.
#[must_use]
pub fn normalize_package_relative_path(raw: &str) -> Option<String> {
    if raw.contains(':') {
        return None;
    }
    normalize_relative_path(raw, ParentTraversal::Reject)
}

/// Normalize a declared bin target within the package root using `/`
/// separators. Excess parent segments are clamped at that root.
#[must_use]
pub fn normalize_bin_path(raw: &str) -> Option<String> {
    let raw = raw.replace('\\', "/");
    if has_windows_drive_prefix(&raw) {
        return None;
    }
    let normalized = normalize_relative_path(&raw, ParentTraversal::Clamp)?;
    (!has_windows_drive_prefix(&normalized)).then_some(normalized)
}

fn has_windows_drive_prefix(path: &str) -> bool {
    path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic()
}

#[derive(Clone, Copy)]
enum ParentTraversal {
    Reject,
    Clamp,
}

fn normalize_relative_path(raw: &str, parent_traversal: ParentTraversal) -> Option<String> {
    if raw.starts_with('/') || raw.contains('\\') {
        return None;
    }
    let mut stack = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() && matches!(parent_traversal, ParentTraversal::Reject) {
                    return None;
                }
            }
            normal => stack.push(normal),
        }
    }
    Some(stack.join("/"))
}
