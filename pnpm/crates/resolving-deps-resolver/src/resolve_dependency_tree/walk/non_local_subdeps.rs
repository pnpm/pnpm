//! Reject host-filesystem and path-based `workspace:` dependencies declared by
//! non-local packages while preserving workspace hooks, `packageExtensions`,
//! `overrides`, `linkWorkspacePackages: deep`, and package-internal
//! `link:<root>/...` links.

pub(crate) use peers_and_overrides::{
    HoistedPeerReachability,
    is_detached_peer_spec_allowed,
    validate_hoisted_peer_spec,
};

use std::collections::HashSet;

use peers_and_overrides::{
    manifest_identity,
    override_authorizes_spec,
    parse_pkg_id_identity,
    record_local_package_peers,
    record_non_local_peers,
};
use pnpm_local_spec::is_non_local_filesystem_subdep_spec;
use pnpm_lockfile::{
    LockfileResolution,
    package_root_link_target,
};
use pnpm_network::redact_and_sanitize;
use pnpm_resolving_resolver_base::ResolveResult;
use serde_json::Value;

use super::{
    ChildEdge,
    ResolveDependencyTreeError,
    TreeCtx,
    WantedDependency,
    lock_recoverable,
    wanted_package_root_link,
};

mod peers_and_overrides;

const CHILD_SPEC_GROUPS: [&str; 2] = ["dependencies", "optionalDependencies"];

/// How a child edge's declaring parent was resolved.
#[derive(Default, Clone, Copy)]
pub(in super::super) enum ParentOrigin<'e> {
    /// Importer root or non-workspace local package.
    #[default]
    RootOrLocal,
    /// Workspace package (`resolved_via == "workspace"`).
    Workspace,
    /// Non-local package (`npm`, `git`, remote tarball, or `link:<root>/...`),
    /// carrying its raw resolver id.
    NonLocal(&'e str),
}

impl<'e> ParentOrigin<'e> {
    pub(in super::super) fn of(result: &'e ResolveResult) -> Self {
        if result.resolved_via == "workspace" {
            Self::Workspace
        } else if is_local_package(result) {
            Self::RootOrLocal
        } else {
            Self::NonLocal(result.id.as_str())
        }
    }

    pub(super) fn is_workspace(self) -> bool {
        matches!(self, Self::Workspace)
    }

    pub(super) fn non_local_id(self) -> Option<&'e str> {
        match self {
            Self::NonLocal(id) => Some(id),
            Self::RootOrLocal | Self::Workspace => None,
        }
    }
}

/// Whether `result` is a local directory, local tarball, or workspace package
/// whose own manifest may declare relative or host-filesystem dependencies.
pub(super) fn is_local_package(result: &ResolveResult) -> bool {
    let raw_id = result.id.as_str();
    if is_package_root_link_spec(raw_id) {
        return false;
    }
    matches!(result.resolution, LockfileResolution::Directory(_))
        || matches!(result.resolved_via.as_str(), "local-filesystem" | "workspace")
        || raw_id.starts_with("file:")
        || raw_id.starts_with("link:")
}

/// Validate a package's manifest after workspace hooks run: reject un-rewritten
/// host-filesystem child dependencies on non-local packages and record peer and
/// hook authorizations for later hoist and edge checks.
pub(super) fn validate_and_record_non_local_manifest(
    ctx: &TreeCtx,
    result: &ResolveResult,
    pre_hook: &Value,
    hooked: &Value,
) -> Result<(), ResolveDependencyTreeError> {
    if is_local_package(result) {
        record_local_package_peers(ctx, result, hooked);
        return Ok(());
    }
    reject_retained_local_specs(ctx, result, pre_hook, hooked)?;
    record_hook_authorized_children(ctx, result, pre_hook, hooked);
    record_non_local_peers(ctx, result, pre_hook, hooked);
    Ok(())
}

/// Defense-in-depth check on a resolved child edge of a non-local manifest:
/// rejects `local-filesystem` and unauthorized `workspace` resolutions that
/// were not rewritten by workspace hooks or `overrides`.
pub(super) fn reject_non_local_filesystem_subdep(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    result: &ResolveResult,
    edge: &ChildEdge<'_>,
) -> Result<(), ResolveDependencyTreeError> {
    let Some(parent_id) = edge.parent_origin.non_local_id() else {
        return Ok(());
    };
    if edge.depth == 0 || wanted_package_root_link(wanted).is_some() {
        return Ok(());
    }
    let alias = wanted.alias.as_deref().unwrap_or_default();
    let specifier = wanted.bare_specifier.as_deref().unwrap_or_default();
    if (is_local_git_pkg_id(parent_id) && is_git_file_spec(specifier))
        || is_hook_authorized_child(&ctx.workspace.policy.local_subdeps, parent_id, alias)
        || override_authorizes_spec(ctx, parse_pkg_id_identity(parent_id), alias, specifier)
    {
        return Ok(());
    }
    if !is_disallowed_child_provenance(ctx, wanted, result, edge.depth) {
        return Ok(());
    }
    Err(local_subdep_error(parent_id, alias, specifier))
}

fn reject_retained_local_specs(
    ctx: &TreeCtx,
    result: &ResolveResult,
    pre_hook: &Value,
    hooked: &Value,
) -> Result<(), ResolveDependencyTreeError> {
    for field in CHILD_SPEC_GROUPS {
        let Some(entries) = pre_hook.get(field).and_then(Value::as_object) else {
            continue;
        };
        reject_retained_field_specs(ctx, result, hooked, field, entries)?;
    }
    Ok(())
}

fn reject_retained_field_specs(
    ctx: &TreeCtx,
    result: &ResolveResult,
    hooked: &Value,
    field: &str,
    entries: &serde_json::Map<String, Value>,
) -> Result<(), ResolveDependencyTreeError> {
    let parent_id = manifest_identity(result, hooked);
    let local_git_parent = is_local_git_parent(result);
    for (alias, raw_value) in entries {
        let Some(raw_spec) = raw_value
            .as_str()
            .filter(|spec| {
                !(local_git_parent && is_git_file_spec(spec))
                    && is_forbidden_manifest_spec(ctx, spec)
            })
        else {
            continue;
        };
        if manifest_dep_spec(hooked, field, alias) == Some(raw_spec)
            && !override_authorizes_spec(ctx, parent_id, alias, raw_spec)
        {
            let parent = describe_non_local_package(result, hooked);
            return Err(local_subdep_error(&parent, alias, raw_spec));
        }
    }
    Ok(())
}

pub(super) fn is_forbidden_manifest_spec(ctx: &TreeCtx, spec: &str) -> bool {
    if is_package_root_link_spec(spec) {
        return false;
    }
    let prefixes = &ctx.workspace.cache.registry_context.registries_by_prefix;
    is_non_local_filesystem_subdep_spec(spec, |prefix| prefixes.contains_key(prefix))
}

fn record_hook_authorized_children(
    ctx: &TreeCtx,
    result: &ResolveResult,
    pre_hook: &Value,
    hooked: &Value,
) {
    let parent_id = manifest_identity(result, hooked);
    let mut authorized = HashSet::new();
    for field in CHILD_SPEC_GROUPS {
        if let Some(entries) = hooked.get(field).and_then(Value::as_object) {
            collect_hook_authorized_field_aliases(
                ctx,
                parent_id,
                (pre_hook, field, entries),
                &mut authorized,
            );
        }
    }
    if !authorized.is_empty() {
        lock_recoverable(&ctx.workspace.policy.local_subdeps.hook_authorized_local_children)
            .entry(result.id.as_str().to_string())
            .or_default()
            .extend(authorized);
    }
}

fn collect_hook_authorized_field_aliases(
    ctx: &TreeCtx,
    parent_id: Option<(&str, Option<&str>)>,
    (pre_hook, field, entries): (&Value, &str, &serde_json::Map<String, Value>),
    authorized: &mut HashSet<String>,
) {
    for (alias, post_value) in entries {
        let Some(post_spec) = post_value.as_str() else {
            continue;
        };
        if manifest_dep_spec(pre_hook, field, alias) != Some(post_spec)
            || override_authorizes_spec(ctx, parent_id, alias, post_spec)
        {
            authorized.insert(alias.clone());
        }
    }
}

fn is_hook_authorized_child(
    local_subdeps: &super::super::workspace_ctx::LocalSubdepAuthorizations,
    parent_id: &str,
    alias: &str,
) -> bool {
    lock_recoverable(&local_subdeps.hook_authorized_local_children)
        .get(parent_id)
        .is_some_and(|aliases| aliases.contains(alias))
}

fn is_disallowed_child_provenance(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    result: &ResolveResult,
    depth: i32,
) -> bool {
    let raw_spec = wanted.bare_specifier.as_deref().unwrap_or_default();
    if is_forbidden_manifest_spec(ctx, raw_spec) {
        return true;
    }
    if wanted_uses_workspace_protocol(raw_spec) {
        return false;
    }
    if result.resolved_via == "local-filesystem" {
        return true;
    }
    if result.resolved_via != "workspace" {
        return false;
    }
    let depth = u32::try_from(depth).unwrap_or(u32::MAX);
    !ctx.options.base.project.link_workspace_packages.enabled_at_depth(depth)
}

fn wanted_uses_workspace_protocol(spec: &str) -> bool {
    spec.get(.."workspace:".len())
        .is_some_and(|head| head.eq_ignore_ascii_case("workspace:"))
}

pub(super) fn manifest_dep_spec<'m>(
    manifest: &'m Value,
    field: &str,
    alias: &str,
) -> Option<&'m str> {
    manifest
        .get(field)
        .and_then(Value::as_object)
        .and_then(|entries| entries.get(alias))
        .and_then(Value::as_str)
}

fn is_package_root_link_spec(spec: &str) -> bool {
    spec.strip_prefix("link:")
        .and_then(package_root_link_target)
        .is_some()
}

fn is_local_git_parent(result: &ResolveResult) -> bool {
    matches!(
        &result.resolution,
        LockfileResolution::Git(git) if has_ascii_prefix(&git.repo, "file:"),
    ) || is_local_git_pkg_id(result.id.as_str())
}

fn is_local_git_pkg_id(pkg_id: &str) -> bool {
    let head = pkg_id.split_once('(').map_or(pkg_id, |(prefix, _)| prefix);
    has_ascii_prefix(head, "git+file:")
        || head
            .rfind('@')
            .is_some_and(|idx| idx >= 1 && has_ascii_prefix(&head[idx + 1..], "git+file:"))
}

fn is_git_file_spec(spec: &str) -> bool {
    let trimmed = spec.trim();
    if has_ascii_prefix(trimmed, "git+file:") {
        return true;
    }
    if !has_ascii_prefix(trimmed, "npm:") {
        return false;
    }
    let after_npm = &trimmed["npm:".len()..];
    has_ascii_prefix(after_npm, "git+file:")
        || after_npm
            .rfind('@')
            .is_some_and(|idx| idx >= 1 && has_ascii_prefix(&after_npm[idx + 1..], "git+file:"))
}

fn has_ascii_prefix(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

pub(super) fn describe_non_local_package(result: &ResolveResult, manifest: &Value) -> String {
    let name = manifest.get("name").and_then(Value::as_str);
    let version = manifest.get("version").and_then(Value::as_str);
    match (name, version) {
        (Some(name), Some(version)) if !name.is_empty() && !version.is_empty() => {
            format!("{name}@{version}")
        }
        (Some(name), _) if !name.is_empty() => name.to_string(),
        _ => result.id.as_str().to_string(),
    }
}

pub(super) fn local_subdep_error(
    parent: &str,
    alias: &str,
    specifier: &str,
) -> ResolveDependencyTreeError {
    ResolveDependencyTreeError::LocalSubdepNotAllowed {
        parent: redact_and_sanitize(parent),
        alias: redact_and_sanitize(alias),
        specifier: redact_and_sanitize(specifier),
    }
}
