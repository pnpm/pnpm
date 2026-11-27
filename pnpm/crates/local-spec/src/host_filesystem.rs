use crate::{
    bare_path_is_unambiguous,
    is_drive_letter_prefix,
};
use pnpm_pkg_name_split::split_name_suffix;

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
    if has_local_file_protocol(spec) || strip_package_name_from_local_spec(spec).is_some() {
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

/// Whether `spec` declared in a non-local package manifest targets the host
/// filesystem or a path-based `workspace:` dependency.
///
/// Assumes valid package-internal `file:` directory paths have already been
/// rewritten to `link:<root>/...` and filtered out by the caller.
#[must_use]
pub fn is_non_local_filesystem_subdep_spec(
    spec: &str,
    is_declared_prefix: impl Fn(&str) -> bool,
) -> bool {
    is_non_local_filesystem_subdep_ref(spec, &is_declared_prefix)
}

fn is_non_local_filesystem_subdep_ref(
    spec: &str,
    is_declared_prefix: &dyn Fn(&str) -> bool,
) -> bool {
    let spec = peel_npm_alias(spec);
    if has_non_local_forbidden_protocol(spec) || strip_package_name_from_local_spec(spec).is_some()
    {
        return true;
    }
    if let Some(rest) = strip_ascii_case_prefix(spec, "workspace:") {
        return is_workspace_filesystem_suffix(rest, is_declared_prefix);
    }
    if is_drive_letter_prefix(spec) {
        let (_, rest) = spec.split_once(':').expect("drive prefix contains a colon");
        return rest.starts_with('/') || rest.starts_with('\\') || !is_declared_prefix(&spec[..1]);
    }
    if bare_path_is_unambiguous(spec) || is_tilde_dir_prefix(spec) {
        return true;
    }
    !spec.contains(':') && is_unprefixed_local_path(spec)
}

/// The local specifier that follows the `<name>@` of a specifier written as
/// a package key, such as the `lib@file:///workspace/lib` that `pnpm deploy`
/// writes into the deployed manifest. `None` unless `spec` is a package name,
/// `@`, and a `file:`, `link:`, `path:`, or `git+file:` specifier.
#[must_use]
pub fn strip_package_name_from_local_spec(spec: &str) -> Option<&str> {
    let (name, rest) = split_name_suffix(spec).ok()?;
    let name = name.as_str();
    let scope_slashes = usize::from(name.starts_with('@'));
    let is_package_name = name.matches('/').count() == scope_slashes
        && !name.ends_with('/')
        && !name.starts_with(['.', '_', '~'])
        && !name.contains([':', '\\']);
    (is_package_name && has_non_local_forbidden_protocol(rest)).then_some(rest)
}

fn peel_npm_alias(mut spec: &str) -> &str {
    for _ in 0..8 {
        let Some(aliased) = strip_ascii_case_prefix(spec, "npm:") else {
            break;
        };
        let Some(index) = aliased
            .rfind('@')
            .filter(|index| *index >= 1)
        else {
            break;
        };
        spec = &aliased[index + 1..];
    }
    spec
}

fn has_non_local_forbidden_protocol(spec: &str) -> bool {
    has_local_file_protocol(spec) || strip_ascii_case_prefix(spec, "path:").is_some()
}

fn is_tilde_path(path: &str) -> bool {
    path == "~" || is_tilde_dir_prefix(path)
}

fn is_tilde_dir_prefix(path: &str) -> bool {
    path.starts_with("~/") || path.starts_with(r"~\")
}

fn is_workspace_filesystem_suffix(rest: &str, is_declared_prefix: &dyn Fn(&str) -> bool) -> bool {
    if is_tilde_dir_prefix(rest)
        || rest.starts_with('.')
        || rest.starts_with('/')
        || rest.contains('\\')
        || is_drive_letter_prefix(rest)
    {
        return true;
    }
    let (Some(_alias), version) = pnpm_workspace_spec::split_alias_version(rest) else {
        return false;
    };
    version != "~" && is_non_local_filesystem_subdep_ref(version, is_declared_prefix)
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
    if is_tilde_dir_prefix(rest) || rest.contains('\\') || is_drive_letter_prefix(rest) {
        return true;
    }
    (rest.starts_with('.') || rest.starts_with('/')) && escapes_workspace_root(importer_dir, rest)
}

/// Whether `rel_path`, interpreted relative to POSIX `importer_dir` (`.` at
/// the workspace root), escapes outside the workspace root.
#[must_use]
pub fn escapes_workspace_root(importer_dir: &str, rel_path: &str) -> bool {
    if is_tilde_path(rel_path)
        || rel_path.starts_with('/')
        || rel_path.contains('\\')
        || is_drive_letter_prefix(rel_path)
    {
        return true;
    }
    let Some(initial_depth) = importer_depth(importer_dir) else {
        return true;
    };
    climbs_above_depth(rel_path, initial_depth)
}

fn importer_depth(importer_dir: &str) -> Option<usize> {
    if is_tilde_path(importer_dir)
        || importer_dir.starts_with('/')
        || importer_dir.contains('\\')
        || is_drive_letter_prefix(importer_dir)
    {
        return None;
    }
    let mut depth = 0;
    for segment in importer_dir.split('/') {
        depth = apply_path_segment(depth, segment)?;
    }
    Some(depth)
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
