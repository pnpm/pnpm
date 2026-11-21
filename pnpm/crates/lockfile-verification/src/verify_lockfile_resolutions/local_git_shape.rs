use pnpm_lockfile::{
    LockfileResolution,
    PackageMetadata,
    PkgNameVerPeer,
    VersionPart,
    is_drive_letter_prefix,
};

use super::{
    file_url_shape::{
        FileShapeContext,
        canonical_slash_path,
        decode_file_url_path,
        file_url_matches_target,
    },
    local_containment::roots::normalize_rel_dir,
    offline_checks::strip_file_url_prefix,
};

const REMOTE_GIT_SCHEMES: [&str; 7] = ["http", "https", "ssh", "git", "rsync", "ftp", "ftps"];

/// Whether `repo` resolves against the local host filesystem when handed to
/// `git` (`file:`, local paths, Windows drives, or schemeless non-SCP paths).
pub(super) fn is_local_git_repo(repo: &str) -> bool {
    let rest = strip_git_plus_prefix(repo.trim());
    if rest.is_empty() {
        return false;
    }
    if strip_file_url_prefix(rest).is_some() {
        return true;
    }
    if let Some((scheme, _)) = rest.split_once("://") {
        return !REMOTE_GIT_SCHEMES
            .iter()
            .any(|remote| scheme.eq_ignore_ascii_case(remote));
    }
    if is_drive_letter_prefix(rest) {
        return true;
    }
    !is_scp_like_remote_repo(rest)
}

fn is_scp_like_remote_repo(repo: &str) -> bool {
    let Some(colon_idx) = repo.find(':') else {
        return false;
    };
    if colon_idx < 2 {
        return false;
    }
    let head = &repo[..colon_idx];
    !head.contains('/') && !head.contains('\\')
}

/// Extract the filesystem path portion of a local Git `repo` or `git+file:`
/// specifier, decoding `file://` URLs to canonical slash paths.
pub(super) fn extract_local_git_target(raw: &str) -> Option<String> {
    if !is_local_git_repo(raw) {
        return None;
    }
    let without_git = strip_git_plus_prefix(raw.trim());
    let before_fragment = without_git
        .split('#')
        .next()
        .unwrap_or(without_git);
    let raw_path = strip_file_url_prefix(before_fragment).unwrap_or(before_fragment);
    if raw_path.starts_with("//") {
        decode_file_url_path(raw_path).or_else(|| Some(canonical_slash_path(raw_path)))
    } else {
        Some(canonical_slash_path(raw_path))
    }
}

/// Raw `key_path` after `git+file:` (before `#`), if `key` is a `git+file:`
/// non-semver package key.
pub(super) fn local_git_key_raw_path(key: &PkgNameVerPeer) -> Option<&str> {
    let VersionPart::NonSemver(raw) = key.suffix.version() else {
        return None;
    };
    let rest = strip_case_prefix(raw, "git+file:")?;
    Some(rest.split('#').next().unwrap_or(rest))
}

pub(super) fn is_local_git_key(key: &PkgNameVerPeer) -> bool {
    local_git_key_raw_path(key).is_some()
}

pub(super) fn has_local_git_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Git(git) => is_local_git_repo(&git.repo),
        LockfileResolution::Variations(variations) => {
            variations.variants.iter().any(|variant| has_local_git_resolution(&variant.resolution))
        }
        _ => false,
    }
}

pub(super) fn is_local_git_package(
    base_key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> bool {
    is_local_git_key(base_key)
        || meta.is_some_and(|entry| has_local_git_resolution(&entry.resolution))
}

pub(super) fn local_git_key_mismatches_target(
    key_path: &str,
    resolution: &LockfileResolution,
    shape_ctx: FileShapeContext<'_>,
) -> bool {
    match resolution {
        LockfileResolution::Git(git) => {
            let Some(repo_target) = extract_local_git_target(&git.repo) else {
                return true;
            };
            if !key_path.starts_with("//") {
                return normalize_rel_dir(key_path) != normalize_rel_dir(&repo_target);
            }
            !file_url_matches_target(key_path, &repo_target, shape_ctx)
        }
        LockfileResolution::Variations(variations) => variations.variants
            .iter()
            .any(|variant| {
                local_git_key_mismatches_target(key_path, &variant.resolution, shape_ctx)
            }),
        _ => true,
    }
}

fn strip_git_plus_prefix(input: &str) -> &str {
    strip_case_prefix(input, "git+").unwrap_or(input)
}

fn strip_case_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    input
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &input[prefix.len()..])
}
