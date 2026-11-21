pub(crate) use reachability::HoistedPeerReachability;

use std::collections::HashSet;

use node_semver::{
    Range,
    Version,
};
use pnpm_matcher::WildcardMatcher;
use pnpm_resolving_resolver_base::ResolveResult;
use reachability::{
    candidate_parent_authorizes_peer,
    lockfile_local_pkg_declares_peer,
    untrusted_peer_parent,
};
use serde_json::Value;

use crate::{
    HoistPeersOptions,
    MissingPeerInfo,
    OverrideEntry,
};

use super::{
    super::super::workspace_ctx::LocalSubdepAuthorizations,
    ResolveDependencyTreeError,
    TreeCtx,
    describe_non_local_package,
    is_forbidden_manifest_spec,
    local_subdep_error,
    lock_recoverable,
    manifest_dep_spec,
};

mod lockfile_closure;
mod reachability;

/// Reject a missing peer whose hoisted specifier resolves against the host
/// filesystem unless authorized for this importer by `overrides`,
/// workspace-root dependencies, a local package reachable in this importer, or
/// per-parent hook/override rewrites on every declaring non-local package.
pub(crate) fn validate_hoisted_peer_spec(
    reachability: &HoistedPeerReachability<'_>,
    hoist_opts: &HoistPeersOptions<'_>,
    (missing, alias, hoisted_spec): (&[(String, MissingPeerInfo)], &str, &str),
) -> Result<(), ResolveDependencyTreeError> {
    if !is_forbidden_manifest_spec(reachability.ctx, hoisted_spec) {
        return Ok(());
    }
    let Some(parent) = untrusted_peer_parent(reachability, alias, hoisted_spec) else {
        return Ok(());
    };
    if hoist_opts_authorize_peer(hoist_opts, missing, alias, hoisted_spec)
        || is_authorized_local_peer_spec(reachability, alias, hoisted_spec)
    {
        return Ok(());
    }
    Err(local_subdep_error(&parent, alias, hoisted_spec))
}

/// Check whether a detached optional-peer candidate probe may read
/// `hoisted_spec`. Unlike [`validate_hoisted_peer_spec`], a candidate outside
/// the importer's current reachable closure is not yet in `reachable_ids`, so a
/// forbidden local specifier requires positive authorization before any
/// detached read touches the host filesystem.
pub(crate) fn is_detached_peer_spec_allowed(
    reachability: &HoistedPeerReachability<'_>,
    hoist_opts: &HoistPeersOptions<'_>,
    declaring_parents: &[(&str, &str)],
    (missing, alias, hoisted_spec): (&[(String, MissingPeerInfo)], &str, &str),
) -> bool {
    if !is_forbidden_manifest_spec(reachability.ctx, hoisted_spec) {
        return true;
    }
    if hoist_opts_authorize_peer(hoist_opts, missing, alias, hoisted_spec)
        || is_authorized_local_peer_spec(reachability, alias, hoisted_spec)
    {
        return true;
    }
    !declaring_parents.is_empty()
        && declaring_parents
            .iter()
            .all(|parent| {
                candidate_parent_authorizes_peer(reachability, *parent, alias, hoisted_spec)
            })
}

pub(super) fn record_local_package_peers(ctx: &TreeCtx, result: &ResolveResult, hooked: &Value) {
    record_pkg_name_ver(&ctx.workspace.policy.local_subdeps, result, hooked);
    let Some(peers) = hooked.get("peerDependencies").and_then(Value::as_object) else {
        return;
    };
    let mut authorized = Vec::new();
    for (alias, value) in peers {
        if let Some(spec) = value.as_str().filter(|spec| is_forbidden_manifest_spec(ctx, spec)) {
            authorized.push((alias.clone(), spec.to_string()));
        }
    }
    if !authorized.is_empty() {
        lock_recoverable(&ctx.workspace.policy.local_subdeps.local_pkg_peer_specs)
            .entry(result.id.as_str().to_string())
            .or_default()
            .extend(authorized);
    }
}

pub(super) fn record_non_local_peers(
    ctx: &TreeCtx,
    result: &ResolveResult,
    pre_hook: &Value,
    hooked: &Value,
) {
    record_pkg_name_ver(&ctx.workspace.policy.local_subdeps, result, hooked);
    let Some(post_peers) = hooked.get("peerDependencies").and_then(Value::as_object) else {
        return;
    };
    let mut authorized = Vec::new();
    let mut untrusted = Vec::new();
    for (alias, post_val) in post_peers {
        if let Some(post_spec) =
            post_val.as_str().filter(|spec| is_forbidden_manifest_spec(ctx, spec))
        {
            classify_non_local_peer_entry(
                ctx,
                (result, pre_hook, hooked),
                (alias, post_spec),
                &mut authorized,
                &mut untrusted,
            );
        }
    }
    store_non_local_peer_entries(
        &ctx.workspace.policy.local_subdeps,
        result.id.as_str(),
        authorized,
        untrusted,
    );
}

fn record_pkg_name_ver(
    local_subdeps: &LocalSubdepAuthorizations,
    result: &ResolveResult,
    manifest: &Value,
) {
    if let Some((name, Some(ver))) = manifest_identity(result, manifest) {
        lock_recoverable(&local_subdeps.pkg_name_ver)
            .insert(result.id.as_str().to_string(), (name.to_string(), ver.to_string()));
    }
}

fn store_non_local_peer_entries(
    local_subdeps: &LocalSubdepAuthorizations,
    pkg_id: &str,
    authorized: Vec<(String, String)>,
    untrusted: Vec<((String, String), String)>,
) {
    if !authorized.is_empty() {
        lock_recoverable(&local_subdeps.authorized_non_local_peer_specs)
            .entry(pkg_id.to_string())
            .or_default()
            .extend(authorized);
    }
    if !untrusted.is_empty() {
        lock_recoverable(&local_subdeps.untrusted_local_peer_parents)
            .entry(pkg_id.to_string())
            .or_default()
            .extend(untrusted);
    }
}

fn classify_non_local_peer_entry(
    ctx: &TreeCtx,
    (result, pre_hook, hooked): (&ResolveResult, &Value, &Value),
    (alias, post_spec): (&str, &str),
    authorized: &mut Vec<(String, String)>,
    untrusted: &mut Vec<((String, String), String)>,
) {
    let key = (alias.to_string(), post_spec.to_string());
    let parent_id = manifest_identity(result, hooked);
    if manifest_dep_spec(pre_hook, "peerDependencies", alias) != Some(post_spec)
        || override_authorizes_spec(ctx, parent_id, alias, post_spec)
    {
        authorized.push(key);
    } else {
        untrusted.push((key, describe_non_local_package(result, hooked)));
    }
}

pub(super) fn manifest_identity<'m>(
    result: &'m ResolveResult,
    manifest: &'m Value,
) -> Option<(&'m str, Option<&'m str>)> {
    let name = manifest.get("name").and_then(Value::as_str);
    let version = manifest.get("version").and_then(Value::as_str);
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        return Some((name, version.filter(|ver| !ver.is_empty())));
    }
    parse_pkg_id_identity(result.id.as_str())
}

pub(super) fn parse_pkg_id_identity(pkg_id: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = pkg_id.split_once('(').map_or(pkg_id, |(head, _)| head);
    let at_index = trimmed
        .rfind('@')
        .filter(|index| *index >= 1)?;
    let name = &trimmed[..at_index];
    let ver = &trimmed[at_index + 1..];
    Some((name, (!ver.is_empty()).then_some(ver)))
}

pub(super) fn override_authorizes_spec(
    ctx: &TreeCtx,
    parent: Option<(&str, Option<&str>)>,
    alias: &str,
    spec: &str,
) -> bool {
    ctx.workspace.policy.parsed_overrides
        .as_deref()
        .is_some_and(|overrides| {
            overrides.iter().any(|entry| override_entry_matches(entry, parent, alias, spec))
        })
}

fn override_entry_matches(
    entry: &OverrideEntry,
    parent: Option<(&str, Option<&str>)>,
    alias: &str,
    spec: &str,
) -> bool {
    if entry.new_bare_specifier != spec || !WildcardMatcher::new(&entry.name).matches(alias) {
        return false;
    }
    if let Some(target_range) = entry.selector_range.as_deref()
        && target_range != "*"
        && target_range != spec
    {
        return false;
    }
    let Some(parent_name) = entry.parent_name.as_deref() else {
        return true;
    };
    let Some((decl_name, decl_ver)) = parent else {
        return false;
    };
    if !WildcardMatcher::new(parent_name).matches(decl_name) {
        return false;
    }
    entry.parent_range
        .as_deref()
        .is_none_or(|range| decl_ver.is_some_and(|ver| version_satisfies_selector(ver, range)))
}

fn version_satisfies_selector(version: &str, selector_range: &str) -> bool {
    if selector_range == "*" || selector_range == version {
        return true;
    }
    Version::parse(version)
        .ok()
        .zip(Range::parse(selector_range).ok())
        .is_some_and(|(parsed_ver, parsed_range)| parsed_range.satisfies(&parsed_ver))
}

fn hoist_opts_authorize_peer(
    hoist_opts: &HoistPeersOptions<'_>,
    missing: &[(String, MissingPeerInfo)],
    alias: &str,
    hoisted_spec: &str,
) -> bool {
    let root_matches = hoist_opts.workspace_root_deps
        .iter()
        .any(|dep| {
            (dep.alias == alias || dep.pkg_name == alias)
                && dep.normalized_bare_specifier.as_deref() == Some(hoisted_spec)
        });
    if root_matches {
        return true;
    }
    let Some(overrider) = hoist_opts.override_bare_specifier else {
        return false;
    };
    let range = missing
        .iter()
        .find_map(|(name, info)| (name == alias).then_some(info.range.as_str()))
        .unwrap_or(hoisted_spec);
    overrider(alias, range, hoist_opts.project_dir).as_deref() == Some(hoisted_spec)
}

fn is_authorized_local_peer_spec(
    reachability: &HoistedPeerReachability<'_>,
    alias: &str,
    spec: &str,
) -> bool {
    let ctx = reachability.ctx;
    let key = (alias.to_string(), spec.to_string());
    if reachable_local_pkg_declares_peer(
        &ctx.workspace.policy.local_subdeps,
        reachability.reachable_ids(),
        &key,
    ) {
        return true;
    }
    override_authorizes_spec(ctx, None, alias, spec)
        || lockfile_local_pkg_declares_peer(reachability, alias, spec)
}

fn reachable_local_pkg_declares_peer(
    local_subdeps: &LocalSubdepAuthorizations,
    reachable_ids: &HashSet<String>,
    key: &(String, String),
) -> bool {
    let local_by_pkg = lock_recoverable(&local_subdeps.local_pkg_peer_specs);
    reachable_ids
        .iter()
        .any(|pkg_id| {
            local_by_pkg
                .get(pkg_id)
                .is_some_and(|specs| specs.contains(key))
        })
}
