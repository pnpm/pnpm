use node_semver::{
    Range,
    Version,
};
use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PackageMetadata,
    PkgNameVerPeer,
    VersionPart,
    is_host_filesystem_specifier,
    is_non_local_filesystem_subdep_spec,
};
use pnpm_matcher::WildcardMatcher;

use super::{
    super::{
        super::{
            file_url_shape::{
                canonical_slash_path,
                decode_file_url_path,
                percent_decode,
            },
            local_git_shape::extract_local_git_target,
            offline_checks::strip_file_url_prefix,
        },
        escapes_above_depth,
        roots::{
            has_inner_parent_segment,
            is_non_relative_target,
            normalize_rel_dir,
        },
    },
    AuthorizedLocalEdge,
    LocalDepField,
    LocalResolutionPolicy,
    parse::{
        extract_spec_local_path,
        relative_posix_path,
    },
};

pub(in super::super) fn target_escapes_workspace(
    policy: &LocalResolutionPolicy,
    rel_path: &str,
) -> bool {
    if is_non_relative_target(rel_path) {
        if let (Some(lockfile_dir), Some(ws_dir)) =
            (policy.scope.lockfile_dir.as_deref(), policy.scope.workspace_dir.as_deref())
            && lockfile_dir.is_absolute()
        {
            return relative_posix_path(ws_dir, rel_path)
                .is_none_or(|rel| rel == ".." || rel.starts_with("../"));
        }
        return true;
    }
    if let Some(depth) = policy.scope.workspace_depth_above_lockfile {
        return escapes_above_depth(rel_path, depth);
    }
    let (Some(lockfile_dir), Some(ws_dir)) =
        (policy.scope.lockfile_dir.as_deref(), policy.scope.workspace_dir.as_deref())
    else {
        return true;
    };
    let Some(lockfile_str) = lockfile_dir.to_str() else {
        return true;
    };
    let joined = format!("{}/{rel_path}", lockfile_str.replace('\\', "/"));
    relative_posix_path(ws_dir, &joined).is_none_or(|rel| rel == ".." || rel.starts_with("../"))
}

pub(in super::super) fn importer_specifier_declares_local(
    policy: &LocalResolutionPolicy,
    alias: &str,
    raw_specifier: &str,
) -> bool {
    let trimmed = raw_specifier.trim();
    if strip_ascii_prefix(trimmed, "workspace:").is_some() {
        return true;
    }
    let resolved = policy.resolve_catalog_spec(alias, trimmed);
    strip_ascii_prefix(resolved, "workspace:").is_some()
        || is_non_local_filesystem_subdep_spec(resolved, |_| false)
        || is_host_filesystem_specifier(resolved, ".", |_| false)
}

pub(in super::super) fn policy_authorizes_importer_dep(
    policy: &LocalResolutionPolicy,
    lockfile: &Lockfile,
    alias: &str,
    declared_spec: &str,
    target: &str,
) -> bool {
    if after_all_resolved_allows(policy, lockfile, target) {
        return true;
    }
    let resolved_decl = policy.resolve_catalog_spec(alias, declared_spec);
    let normalized = normalize_target_for_match(target);
    policy.authorized_edges
        .iter()
        .flat_map(|edges| edges.iter())
        .any(|edge| {
            edge.parent_pkg_key.is_none()
                && edge.parent_name
                    .as_deref()
                    .is_none_or(|name| name == "*")
                && WildcardMatcher::new(&edge.child_alias).matches(alias)
                && edge_matches_child_range(edge, Some(resolved_decl))
                && edge.targets.contains(&normalized)
        })
}

pub(in super::super) fn policy_authorizes_link(
    policy: &LocalResolutionPolicy,
    lockfile: &Lockfile,
    (parent_key, parent_meta): (&PkgNameVerPeer, Option<&PackageMetadata>),
    (field, alias, target): (LocalDepField, &str, &str),
) -> bool {
    if after_all_resolved_allows(policy, lockfile, target) {
        return true;
    }
    let base_parent = parent_key.without_peer();
    let parent_name = base_parent.name.to_string();
    let parent_ver = parent_semver_or_version(&base_parent, parent_meta);
    let declared = declared_child_spec(policy, (&base_parent, parent_meta), field, alias);
    let normalized = normalize_target_for_match(target);
    matching_edges(policy, (&base_parent, &parent_name, parent_ver.as_deref()), (alias, declared))
        .any(|edge| edge.targets.contains(&normalized))
}

pub(in super::super) fn policy_authorizes_package(
    policy: &LocalResolutionPolicy,
    lockfile: &Lockfile,
    (parent_key, parent_meta): (&PkgNameVerPeer, Option<&PackageMetadata>),
    (field, alias, child_key, child_meta): (
        LocalDepField,
        &str,
        &PkgNameVerPeer,
        Option<&PackageMetadata>,
    ),
) -> bool {
    let Some(target) = extract_local_package_target(child_key, child_meta) else {
        return false;
    };
    if after_all_resolved_allows(policy, lockfile, &target) {
        return true;
    }
    let base_parent = parent_key.without_peer();
    let parent_name = base_parent.name.to_string();
    let parent_ver = parent_semver_or_version(&base_parent, parent_meta);
    let declared = declared_child_spec(policy, (&base_parent, parent_meta), field, alias);
    let normalized = normalize_target_for_match(&target);
    matching_edges(policy, (&base_parent, &parent_name, parent_ver.as_deref()), (alias, declared))
        .any(|edge| edge.targets.contains(&normalized))
}

/// The parent's pristine range for `alias` under `field`. A resolved peer is
/// recorded among the snapshot's dependencies, so a parent that declares
/// `alias` only as a peer falls back to its `peerDependencies` range.
fn declared_child_spec<'p>(
    policy: &'p LocalResolutionPolicy,
    (base_parent, parent_meta): (&PkgNameVerPeer, Option<&PackageMetadata>),
    field: LocalDepField,
    alias: &str,
) -> Option<&'p str> {
    let declared_peer = || {
        parent_meta
            .and_then(|meta| meta.peer_dependencies.as_ref())
            .is_some_and(|peers| peers.contains_key(alias))
    };
    policy
        .pristine_declared_spec(base_parent, field, alias.to_owned())
        .or_else(|| {
            declared_peer()
                .then(|| {
                    policy.pristine_declared_spec(
                        base_parent,
                        LocalDepField::PeerDependencies,
                        alias.to_owned(),
                    )
                })
                .flatten()
        })
        .map(|spec| policy.resolve_catalog_spec(alias, spec))
}

pub(in super::super) fn policy_authorizes_peer_spec(
    policy: &LocalResolutionPolicy,
    lockfile: &Lockfile,
    (pkg_key, meta): (&PkgNameVerPeer, &PackageMetadata),
    alias: &str,
    raw_spec: &str,
) -> bool {
    let Some(spec_path) = extract_spec_local_path(raw_spec) else {
        return false;
    };
    if after_all_resolved_allows(policy, lockfile, spec_path) {
        return true;
    }
    let base_parent = pkg_key.without_peer();
    let parent_ver = parent_semver_or_version(&base_parent, Some(meta));
    let declared = policy
        .pristine_declared_spec(&base_parent, LocalDepField::PeerDependencies, alias.to_owned())
        .map_or(raw_spec, |spec| policy.resolve_catalog_spec(alias, spec));
    let normalized = normalize_target_for_match(spec_path);
    matching_edges(
        policy,
        (&base_parent, &base_parent.name.to_string(), parent_ver.as_deref()),
        (alias, Some(declared)),
    )
    .any(|edge| edge.targets.contains(&normalized))
}

fn normalize_target_for_match(target: &str) -> String {
    if target.starts_with("//")
        && let Some(decoded) = decode_file_url_path(target)
    {
        let steps_back = percent_decode(target).is_none_or(|raw| has_inner_parent_segment(&raw));
        return if steps_back { format!("\0{target}") } else { decoded };
    }
    let slash = target.replace('\\', "/");
    if has_inner_parent_segment(&slash) {
        return format!("\0{slash}");
    }
    if slash.starts_with('/') || pnpm_lockfile::is_drive_letter_prefix(&slash) {
        canonical_slash_path(&slash)
    } else {
        normalize_rel_dir(&slash)
    }
}

fn after_all_resolved_allows(
    policy: &LocalResolutionPolicy,
    lockfile: &Lockfile,
    target: &str,
) -> bool {
    let lockfile_has_hook = lockfile.pnpmfile_checksum.is_some()
        || lockfile.untracked_pnpmfile_read_package_hook().unwrap_or(false);
    policy.has_after_all_resolved_hook
        && lockfile_has_hook
        && !target_escapes_workspace(policy, target)
}

fn matching_edges<'p>(
    policy: &'p LocalResolutionPolicy,
    (base_parent, parent_name, parent_ver): (&'p PkgNameVerPeer, &'p str, Option<&'p str>),
    (alias, declared_child_spec): (&'p str, Option<&'p str>),
) -> impl Iterator<Item = &'p AuthorizedLocalEdge> + 'p {
    policy.authorized_edges
        .iter()
        .flat_map(|edges| edges.iter())
        .filter(move |edge| {
            edge.parent_pkg_key
                .as_ref()
                .is_none_or(|key| key == base_parent)
                && WildcardMatcher::new(&edge.child_alias).matches(alias)
                && edge_matches_child_range(edge, declared_child_spec)
                && edge_matches_parent(edge, parent_name, parent_ver)
        })
}

fn edge_matches_child_range(edge: &AuthorizedLocalEdge, declared_spec: Option<&str>) -> bool {
    let Some(range) = edge.child_range.as_deref() else {
        return true;
    };
    if range.is_empty() || range == "*" {
        return true;
    }
    declared_spec.is_some_and(|spec| {
        spec == range
            || Range::parse(range)
                .ok()
                .zip(Range::parse(spec).ok())
                .is_some_and(|(sel_range, decl_range)| sel_range.allows_any(&decl_range))
    })
}

fn edge_matches_parent(
    edge: &AuthorizedLocalEdge,
    parent_name: &str,
    parent_ver: Option<&str>,
) -> bool {
    let Some(sel_name) = edge.parent_name.as_deref() else {
        return true;
    };
    if !WildcardMatcher::new(sel_name).matches(parent_name) {
        return false;
    }
    edge.parent_range
        .as_deref()
        .is_none_or(|range| parent_ver.is_some_and(|ver| version_satisfies(ver, range)))
}

fn version_satisfies(version: &str, selector_range: &str) -> bool {
    if selector_range == "*" || selector_range == version {
        return true;
    }
    Version::parse(version)
        .ok()
        .zip(Range::parse(selector_range).ok())
        .is_some_and(|(parsed_ver, parsed_range)| parsed_range.satisfies(&parsed_ver))
}

pub(in super::super) fn parent_semver_or_version(
    key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> Option<String> {
    key.suffix
        .version_semver()
        .map(ToString::to_string)
        .or_else(|| meta.and_then(|entry| entry.version.clone()))
}

pub(in super::super) fn extract_local_package_target(
    key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> Option<String> {
    match meta.map(|entry| &entry.resolution) {
        Some(LockfileResolution::Directory(dir)) => Some(dir.directory.clone()),
        Some(LockfileResolution::Tarball(tarball)) => {
            strip_file_url_prefix(&tarball.tarball).map(str::to_string)
        }
        Some(LockfileResolution::Git(git)) => extract_local_git_target(&git.repo),
        _ => match key.suffix.version() {
            VersionPart::File(path) => Some(path.clone()),
            VersionPart::NonSemver(raw) => extract_local_git_target(raw),
            _ => None,
        },
    }
}

fn strip_ascii_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    input
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &input[prefix.len()..])
}
