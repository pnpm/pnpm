use std::path::Path;

use pnpm_config_parse_overrides::parse_pkg_and_parent_selector;
use pnpm_lockfile::{
    is_drive_letter_prefix,
    is_host_filesystem_specifier,
    is_non_local_filesystem_subdep_spec,
    strip_package_name_from_local_spec,
};

use super::{
    super::{
        super::file_url_shape::{
            canonical_slash_path,
            decode_file_url_path,
        },
        roots::{
            is_non_relative_target,
            normalize_rel_dir,
        },
    },
    AuthorizedLocalEdge,
};

pub(super) fn parse_override_edge<'a>(
    selector: &str,
    spec: &'a str,
    (overrides_root_dir, lockfile_dir): (Option<&Path>, Option<&Path>),
    resolve_catalog: &impl Fn(&str, &str) -> Option<&'a str>,
) -> Option<AuthorizedLocalEdge> {
    if selector == "*" {
        let resolved = deref_catalog(spec, "*", resolve_catalog);
        let targets = build_spec_targets(resolved, overrides_root_dir, lockfile_dir)?;
        return Some(AuthorizedLocalEdge {
            parent_pkg_key: None,
            parent_name: None,
            parent_range: None,
            child_alias: "*".to_string(),
            child_range: None,
            targets,
        });
    }
    let (parent_pkg, target_pkg) = parse_pkg_and_parent_selector(selector).ok()?;
    let resolved = deref_catalog(spec, &target_pkg.name, resolve_catalog);
    let targets = build_spec_targets(resolved, overrides_root_dir, lockfile_dir)?;
    Some(AuthorizedLocalEdge {
        parent_pkg_key: None,
        parent_name: parent_pkg.as_ref().map(|parent| parent.name.clone()),
        parent_range: parent_pkg
            .as_ref()
            .and_then(|parent| parent.bare_specifier.clone()),
        child_alias: target_pkg.name,
        child_range: target_pkg.bare_specifier,
        targets,
    })
}

pub(super) fn deref_catalog<'a>(
    spec: &'a str,
    target_name: &str,
    resolve_catalog: &impl Fn(&str, &str) -> Option<&'a str>,
) -> &'a str {
    let Some(rest) = spec.strip_prefix("catalog:") else {
        return spec;
    };
    let catalog_name = if rest.is_empty() { "default" } else { rest };
    resolve_catalog(catalog_name, target_name).unwrap_or(spec)
}

pub(super) fn parse_extension_edge(
    parent_selector: &str,
    child_alias: &str,
    spec: &str,
    lockfile_dir: Option<&Path>,
) -> Option<AuthorizedLocalEdge> {
    let targets = build_spec_targets(spec, lockfile_dir, lockfile_dir)?;
    if parent_selector == "*" {
        return Some(AuthorizedLocalEdge {
            parent_pkg_key: None,
            parent_name: None,
            parent_range: None,
            child_alias: child_alias.to_string(),
            child_range: None,
            targets,
        });
    }
    let (_, target_pkg) = parse_pkg_and_parent_selector(parent_selector).ok()?;
    Some(AuthorizedLocalEdge {
        parent_pkg_key: None,
        parent_name: Some(target_pkg.name),
        parent_range: target_pkg.bare_specifier,
        child_alias: child_alias.to_string(),
        child_range: None,
        targets,
    })
}

pub(super) fn build_spec_targets(
    spec: &str,
    base_dir: Option<&Path>,
    lockfile_dir: Option<&Path>,
) -> Option<Vec<String>> {
    if !is_non_local_filesystem_subdep_spec(spec, |_| false)
        && !is_host_filesystem_specifier(spec, ".", |_| false)
    {
        return None;
    }
    let decoded = decode_raw_spec_path(extract_spec_local_path(spec)?);
    let is_abs = decoded.starts_with('/') || is_drive_letter_prefix(&decoded);
    if !is_abs && !is_non_relative_target(&decoded) {
        return rebase_relative_target(&decoded, base_dir, lockfile_dir);
    }
    Some(build_non_relative_targets(&decoded, is_abs, lockfile_dir))
}

fn decode_raw_spec_path(raw_path: &str) -> String {
    if raw_path.starts_with("//") {
        decode_file_url_path(raw_path).unwrap_or_else(|| raw_path.replace('\\', "/"))
    } else {
        raw_path.replace('\\', "/")
    }
}

fn build_non_relative_targets(
    decoded: &str,
    is_abs: bool,
    lockfile_dir: Option<&Path>,
) -> Vec<String> {
    let primary = if is_abs { canonical_slash_path(decoded) } else { normalize_rel_dir(decoded) };
    let mut targets = vec![primary];
    if is_abs
        && let Some(lf_dir) = lockfile_dir
        && let Some(rel) = relative_posix_path(lf_dir, decoded)
    {
        let norm_rel = normalize_rel_dir(&rel);
        if !targets.contains(&norm_rel) {
            targets.push(norm_rel);
        }
    }
    targets
}

fn rebase_relative_target(
    slash_rel: &str,
    base_dir: Option<&Path>,
    lockfile_dir: Option<&Path>,
) -> Option<Vec<String>> {
    if let (Some(base), Some(lf_dir)) = (base_dir, lockfile_dir)
        && base != lf_dir
        && let Some(base_str) = base.to_str()
    {
        let joined = format!("{}/{slash_rel}", base_str.replace('\\', "/"));
        let rebased = relative_posix_path(lf_dir, &joined)
            .map(|rel| normalize_rel_dir(&rel))
            .unwrap_or_else(|| canonical_slash_path(&joined));
        return Some(vec![rebased]);
    }
    Some(vec![normalize_rel_dir(slash_rel)])
}

#[derive(Debug, PartialEq, Eq)]
enum VolumeRoot {
    Posix,
    Drive(char),
    Unc(String, String),
}

impl VolumeRoot {
    const fn min_segments(&self) -> usize {
        match self {
            Self::Posix => 0,
            Self::Drive(_) => 1,
            Self::Unc(_, _) => 2,
        }
    }
}

pub(crate) fn relative_posix_path(base: &Path, target_str: &str) -> Option<String> {
    let base_slash = base.to_str()?.replace('\\', "/");
    let target_slash = target_str.replace('\\', "/");
    let base_vol = parse_volume_root(&base_slash)?;
    let target_vol = parse_volume_root(&target_slash)?;
    if base_vol != target_vol {
        return None;
    }
    let min_len = base_vol.min_segments();
    let case_insensitive = !matches!(base_vol, VolumeRoot::Posix);
    let base_parts = normalized_segments(&base_slash, min_len)?;
    let target_parts = normalized_segments(&target_slash, min_len)?;
    let common =
        base_parts
            .iter()
            .zip(target_parts.iter())
            .take_while(|(left, right)| {
                if case_insensitive { left.eq_ignore_ascii_case(right) } else { left == right }
            })
            .count();
    let mut out: Vec<&str> = vec![".."; base_parts.len() - common];
    out.extend_from_slice(&target_parts[common..]);
    Some(if out.is_empty() { ".".to_string() } else { out.join("/") })
}

fn parse_volume_root(slash_path: &str) -> Option<VolumeRoot> {
    if slash_path.starts_with("//") && !slash_path.starts_with("///") {
        let rest = &slash_path[2..];
        let (server, after_server) = rest.split_once('/')?;
        let share = after_server
            .split('/')
            .next()
            .unwrap_or("");
        if server.is_empty()
            || share.is_empty()
            || matches!(server, "." | "..")
            || matches!(share, "." | "..")
        {
            return None;
        }
        return Some(VolumeRoot::Unc(server.to_ascii_lowercase(), share.to_ascii_lowercase()));
    }
    if is_drive_letter_prefix(slash_path)
        && (slash_path.len() == 2 || slash_path.as_bytes().get(2) == Some(&b'/'))
    {
        let drive = slash_path
            .chars()
            .next()?
            .to_ascii_uppercase();
        return Some(VolumeRoot::Drive(drive));
    }
    slash_path.starts_with('/').then_some(VolumeRoot::Posix)
}

fn normalized_segments(slash_path: &str, min_len: usize) -> Option<Vec<&str>> {
    let mut stack: Vec<&str> = Vec::new();
    for segment in slash_path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if stack.len() <= min_len {
                return None;
            }
            stack.pop();
        } else {
            stack.push(segment);
        }
    }
    (stack.len() >= min_len).then_some(stack)
}

pub(super) fn extract_spec_local_path(spec: &str) -> Option<&str> {
    let (unwrapped, was_pkg_prefixed) = unwrap_npm_spec_layers(spec)?;
    let spec = match unwrap_workspace_spec_layer(unwrapped) {
        WorkspaceUnwrap::DirectPath(path) => return Some(path),
        WorkspaceUnwrap::Inner(inner) => inner,
    };
    extract_prefixed_or_bare_local_path(spec, was_pkg_prefixed)
}

fn unwrap_npm_spec_layers(mut spec: &str) -> Option<(&str, bool)> {
    for _ in 0..8 {
        let Some(rest) = strip_case_prefix(spec, "npm:") else { break };
        let idx = rest.rfind('@').filter(|idx| *idx >= 1)?;
        spec = &rest[idx + 1..];
    }
    if let Some(inner) = strip_package_name_from_local_spec(spec) {
        return Some((inner, true));
    }
    Some((spec, false))
}

enum WorkspaceUnwrap<'a> {
    DirectPath(&'a str),
    Inner(&'a str),
}

fn unwrap_workspace_spec_layer(spec: &str) -> WorkspaceUnwrap<'_> {
    let Some(rest) = strip_case_prefix(spec, "workspace:") else {
        return WorkspaceUnwrap::Inner(spec);
    };
    match rest
        .rfind('@')
        .filter(|idx| *idx >= 1 && !rest.starts_with('.'))
    {
        Some(idx) => WorkspaceUnwrap::Inner(&rest[idx + 1..]),
        None => WorkspaceUnwrap::DirectPath(rest),
    }
}

fn extract_prefixed_or_bare_local_path(spec: &str, was_pkg_prefixed: bool) -> Option<&str> {
    if let Some(rest) = strip_case_prefix(spec, "git+file:") {
        return Some(rest.split('#').next().unwrap_or(rest));
    }
    if let Some(rest) =
        strip_case_prefix(spec, "file:").or_else(|| strip_case_prefix(spec, "link:"))
    {
        if was_pkg_prefixed && rest.starts_with("//") && rest.ends_with(')') {
            return Some(rest.split('(').next().unwrap_or(rest));
        }
        return Some(rest);
    }
    if is_drive_letter_prefix(spec) && matches!(spec.as_bytes().get(2), Some(b'/' | b'\\')) {
        return Some(spec);
    }
    (!spec.contains(':')).then_some(spec)
}

fn strip_case_prefix<'a>(spec: &'a str, prefix: &str) -> Option<&'a str> {
    spec.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &spec[prefix.len()..])
}
