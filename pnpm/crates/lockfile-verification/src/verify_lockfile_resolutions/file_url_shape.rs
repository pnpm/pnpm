use std::path::Path;

use pnpm_lockfile::is_drive_letter_prefix;

use super::local_containment::roots::is_non_relative_target;

/// Context used to verify whether a `file://` URL package key matches a
/// local `directory` or `file:` tarball resolution target.
#[derive(Default, Clone, Copy)]
pub(super) struct FileShapeContext<'a> {
    pub(super) lockfile_dir: Option<&'a Path>,
    pub(super) allow_deploy_file_url_keys: bool,
}

pub(super) fn file_url_matches_target(
    key_path: &str,
    target: &str,
    ctx: FileShapeContext<'_>,
) -> bool {
    if let Some(lockfile_dir) = ctx.lockfile_dir {
        return resolved_file_url_matches(key_path, target, lockfile_dir);
    }
    if is_non_relative_target(target) {
        return decode_file_url_path(key_path)
            .is_some_and(|from_url| from_url == canonical_slash_path(target));
    }
    ctx.allow_deploy_file_url_keys
        && (key_path.starts_with("///") || (key_path.starts_with("//") && key_path != target))
}

fn resolved_file_url_matches(key_path: &str, target: &str, lockfile_dir: &Path) -> bool {
    let Some(from_url) = decode_file_url_path(key_path) else {
        return false;
    };
    let from_target = resolve_against_base(lockfile_dir, target);
    if from_url == from_target {
        return true;
    }
    canonicalized_paths_match(Path::new(&from_url), &lockfile_dir.join(target))
}

pub(super) fn decode_file_url_path(key_path: &str) -> Option<String> {
    if !key_path.starts_with("//") {
        return None;
    }
    let decoded = percent_decode(key_path)?;
    if let Some(after_three) = strip_local_file_authority(&decoded) {
        let raw = if is_drive_letter_prefix(after_three) {
            after_three
        } else {
            return Some(canonical_slash_path(&format!("/{after_three}")));
        };
        return Some(canonical_slash_path(raw));
    }
    Some(canonical_slash_path(&decoded))
}

fn strip_local_file_authority(decoded: &str) -> Option<&str> {
    if let Some(after_three) = decoded.strip_prefix("///") {
        return Some(after_three);
    }
    let prefix = "//localhost/";
    decoded
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &decoded[prefix.len()..])
}

pub(super) fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            let hex = input.get(idx + 1..idx + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn resolve_against_base(base: &Path, target: &str) -> String {
    let target_slash = target.replace('\\', "/");
    if target_slash.starts_with('/') || is_drive_letter_prefix(&target_slash) {
        return canonical_slash_path(&target_slash);
    }
    let base_slash = base.to_string_lossy().replace('\\', "/");
    canonical_slash_path(&format!("{base_slash}/{target_slash}"))
}

pub(super) fn canonical_slash_path(raw: &str) -> String {
    let normalized = raw.replace('\\', "/");
    let (prefix, rest) = split_path_prefix(&normalized);
    let mut stack: Vec<&str> = Vec::new();
    for segment in rest.split('/') {
        apply_path_segment(&mut stack, segment, prefix.is_empty() || prefix.ends_with(':'));
    }
    if prefix.is_empty() {
        return if stack.is_empty() { ".".to_string() } else { stack.join("/") };
    }
    format!("{prefix}{}", stack.join("/"))
}

fn split_path_prefix(slash_path: &str) -> (String, &str) {
    if is_drive_letter_prefix(slash_path) {
        let drive = slash_path.as_bytes()[0].to_ascii_uppercase() as char;
        let rest = &slash_path[2..];
        // `C:dir` resolves against the drive's current directory, so it
        // keeps a prefix distinct from the anchored `C:/dir`.
        return match rest.strip_prefix('/') {
            Some(anchored) => (format!("{drive}:/"), anchored.trim_start_matches('/')),
            None => (format!("{drive}:"), rest),
        };
    }
    if let Some(rest) = slash_path.strip_prefix("//")
        && !rest.starts_with('/')
    {
        return ("//".to_string(), rest);
    }
    if slash_path.starts_with('/') {
        return ("/".to_string(), slash_path.trim_start_matches('/'));
    }
    (String::new(), slash_path)
}

fn apply_path_segment<'s>(stack: &mut Vec<&'s str>, segment: &'s str, relative: bool) {
    if segment.is_empty() || segment == "." {
        return;
    }
    if segment != ".." {
        stack.push(segment);
    } else if stack
        .last()
        .is_some_and(|last| *last != "..")
    {
        stack.pop();
    } else if relative {
        stack.push("..");
    }
}

#[cfg(not(target_family = "wasm"))]
pub(super) fn canonicalized_paths_match(left: &Path, right: &Path) -> bool {
    std::fs::canonicalize(left)
        .ok()
        .zip(std::fs::canonicalize(right).ok())
        .is_some_and(|(left, right)| left == right)
}

#[cfg(target_family = "wasm")]
pub(super) const fn canonicalized_paths_match(_left: &Path, _right: &Path) -> bool {
    false
}
