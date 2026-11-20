use crate::{
    bare_path_is_unambiguous,
    is_drive_letter_prefix,
};

/// Whether `spec`, declared by an importer at POSIX-relative `importer_dir`
/// (`.` for the workspace root), resolves against the host filesystem outside
/// an in-memory workspace package map.
///
/// Covers:
/// - `file:`, `link:`, and `git+file:` specifiers;
/// - `workspace:` path specifiers that escape the workspace root;
/// - unambiguous bare paths (`./`, `../`, `/`, `\`, `~/`, `~\`);
/// - Windows drive specifiers (`C:/...`, `C:\...`, or undeclared `C:...` that
///   is not a configured named-registry prefix);
/// - scheme-less multi-segment or traversal paths (`packages/a/b`, `a/../b`)
///   that fall past hosted-git shorthand (`owner/repo`) into the local path
///   resolver.
#[must_use]
pub fn is_host_filesystem_specifier(
    spec: &str,
    importer_dir: &str,
    is_declared_prefix: impl Fn(&str) -> bool,
) -> bool {
    if has_local_file_protocol(spec) {
        return true;
    }
    if let Some(rest) = strip_ascii_case_prefix(spec, "workspace:") {
        return workspace_spec_escapes_root(rest, importer_dir);
    }
    if let Some(aliased) = strip_ascii_case_prefix(spec, "npm:") {
        return npm_alias_wraps_local_protocol(aliased);
    }
    if is_drive_letter_prefix(spec) {
        let (_, rest) = spec.split_once(':').expect("drive prefix contains a colon");
        return rest.starts_with('/') || rest.starts_with('\\') || !is_declared_prefix(&spec[..1]);
    }
    if bare_path_is_unambiguous(spec) {
        return true;
    }
    !spec.contains(':') && is_unprefixed_local_path(spec)
}

fn has_local_file_protocol(spec: &str) -> bool {
    strip_ascii_case_prefix(spec, "file:").is_some()
        || strip_ascii_case_prefix(spec, "link:").is_some()
        || strip_ascii_case_prefix(spec, "git+file:").is_some()
}

fn npm_alias_wraps_local_protocol(aliased: &str) -> bool {
    match aliased.rfind('@') {
        Some(index) if index >= 1 => has_local_file_protocol(&aliased[index + 1..]),
        _ => false,
    }
}

fn workspace_spec_escapes_root(rest: &str, importer_dir: &str) -> bool {
    if rest.starts_with("~/") || rest.contains('\\') || is_drive_letter_prefix(rest) {
        return true;
    }
    (rest.starts_with('.') || rest.starts_with('/')) && escapes_workspace_root(importer_dir, rest)
}

fn escapes_workspace_root(importer_dir: &str, rel_path: &str) -> bool {
    if rel_path.starts_with('/') || rel_path.contains('\\') || is_drive_letter_prefix(rel_path) {
        return true;
    }
    climbs_above_depth(rel_path, importer_depth(importer_dir))
}

fn importer_depth(importer_dir: &str) -> usize {
    if importer_dir == "." || importer_dir.is_empty() {
        return 0;
    }
    importer_dir
        .split('/')
        .filter(|segment| !segment.is_empty())
        .filter(|segment| *segment != ".")
        .count()
}

fn climbs_above_depth(rel_path: &str, initial_depth: usize) -> bool {
    let mut depth = initial_depth;
    for segment in rel_path.split('/') {
        let Some(next) = apply_path_segment(depth, segment) else {
            return true;
        };
        depth = next;
    }
    false
}

fn apply_path_segment(depth: usize, segment: &str) -> Option<usize> {
    match segment {
        "" | "." => Some(depth),
        ".." => depth.checked_sub(1),
        _ => Some(depth + 1),
    }
}

/// Scheme-less path shapes that neither the npm resolver nor hosted-git
/// shorthand (`owner/repo[#ref]`) claims, so they fall through to the local
/// path resolver.
fn is_unprefixed_local_path(spec: &str) -> bool {
    let path_part = spec.split_once('#').map_or(spec, |(head, _)| head);
    if path_part.contains('\\') {
        return true;
    }
    if !path_part.contains('/') || path_part.starts_with('@') {
        return false;
    }
    let mut segments = 0usize;
    for segment in path_part.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return true;
        }
        segments += 1;
    }
    segments >= 3
}

fn strip_ascii_case_prefix<'spec>(spec: &'spec str, prefix: &str) -> Option<&'spec str> {
    let head = spec.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &spec[prefix.len()..])
}
