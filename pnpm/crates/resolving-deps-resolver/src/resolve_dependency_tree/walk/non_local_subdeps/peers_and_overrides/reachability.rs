use std::{
    cell::OnceCell,
    collections::HashSet,
};

use pnpm_lockfile::{
    PackageMetadata,
    PkgNameVerPeer,
    VersionPart,
    package_root_link_target,
};

use crate::{
    DirectDep,
    resolve_dependency_tree::workspace_ctx::LocalSubdepAuthorizations,
};

use super::{
    TreeCtx,
    lock_recoverable,
    lockfile_closure::importer_reachable_lockfile_packages,
    override_authorizes_spec,
};

/// Lazily computed per-importer reachability state shared across a peer-hoist
/// validation batch so semver-only fast paths perform zero graph work and
/// multiple local peer checks reuse one traversal.
pub(crate) struct HoistedPeerReachability<'a> {
    pub(super) ctx: &'a TreeCtx,
    direct_deps: &'a [DirectDep],
    reachable_ids: OnceCell<HashSet<String>>,
    lockfile_local: OnceCell<HashSet<PkgNameVerPeer>>,
    lockfile_all: OnceCell<HashSet<PkgNameVerPeer>>,
}

impl<'a> HoistedPeerReachability<'a> {
    /// Whether `spec` reads the host filesystem, which only an authorized
    /// declarer may hoist.
    pub(crate) fn is_forbidden_spec(&self, spec: &str) -> bool {
        super::super::is_forbidden_manifest_spec(self.ctx, spec)
    }

    pub(crate) const fn new(ctx: &'a TreeCtx, direct_deps: &'a [DirectDep]) -> Self {
        Self {
            ctx,
            direct_deps,
            reachable_ids: OnceCell::new(),
            lockfile_local: OnceCell::new(),
            lockfile_all: OnceCell::new(),
        }
    }

    pub(super) fn reachable_ids(&self) -> &HashSet<String> {
        self.reachable_ids.get_or_init(|| {
            let reachable_tree = self.ctx.snapshot_reachable_from(self.direct_deps.to_vec());
            reachable_tree.packages
                .values()
                .map(|pkg| pkg.result().id.as_str().to_string())
                .collect()
        })
    }

    pub(super) fn lockfile_reachable(&self, local_only: bool) -> &HashSet<PkgNameVerPeer> {
        let slot = if local_only { &self.lockfile_local } else { &self.lockfile_all };
        slot.get_or_init(|| {
            let Some(lockfile) = self.ctx.workspace.wanted_lockfile() else {
                return HashSet::new();
            };
            let aliases: HashSet<&str> = self.direct_deps
                .iter()
                .filter(|dep| !local_only || is_local_direct_dep_id(&dep.id))
                .map(|dep| dep.alias.as_str())
                .collect();
            importer_reachable_lockfile_packages(
                lockfile,
                &self.ctx.importer.id,
                Some(&aliases),
                local_only,
            )
        })
    }
}

fn is_local_direct_dep_id(raw_id: &str) -> bool {
    if raw_id
        .strip_prefix("link:")
        .and_then(package_root_link_target)
        .is_some()
    {
        return false;
    }
    raw_id.starts_with("file:")
        || raw_id.starts_with("link:")
        || raw_id.contains("@file:")
        || raw_id.contains("@link:")
}

pub(super) fn lockfile_local_pkg_declares_peer(
    reachability: &HoistedPeerReachability<'_>,
    alias: &str,
    spec: &str,
) -> bool {
    let Some(lockfile) = reachability.ctx.workspace.wanted_lockfile() else {
        return false;
    };
    let Some(packages) = lockfile.packages.as_ref() else {
        return false;
    };
    let reachable = reachability.lockfile_reachable(true);
    packages
        .iter()
        .any(|(pkg_key, meta)| {
            reachable.contains(pkg_key)
                && meta.peer_dependencies
                    .as_ref()
                    .and_then(|peers| peers.get(alias))
                    .is_some_and(|declared| declared == spec)
        })
}

pub(super) fn untrusted_peer_parent(
    reachability: &HoistedPeerReachability<'_>,
    alias: &str,
    spec: &str,
) -> Option<String> {
    let key = (alias.to_string(), spec.to_string());
    if let Some(parent) = in_memory_untrusted_peer_parent(
        &reachability.ctx.workspace.policy.local_subdeps,
        reachability.reachable_ids(),
        &key,
    ) {
        return Some(parent);
    }
    lockfile_non_local_peer_parent(reachability, alias, spec)
}

fn in_memory_untrusted_peer_parent(
    local_subdeps: &LocalSubdepAuthorizations,
    reachable_ids: &HashSet<String>,
    key: &(String, String),
) -> Option<String> {
    let untrusted = lock_recoverable(&local_subdeps.untrusted_local_peer_parents);
    reachable_ids
        .iter()
        .find_map(|pkg_id| {
            untrusted
                .get(pkg_id)
                .and_then(|map| map.get(key))
                .cloned()
        })
}

pub(super) fn candidate_parent_authorizes_peer(
    reachability: &HoistedPeerReachability<'_>,
    (parent_name, parent_version): (&str, &str),
    alias: &str,
    spec: &str,
) -> bool {
    if override_authorizes_spec(
        reachability.ctx,
        Some((parent_name, Some(parent_version))),
        alias,
        spec,
    ) {
        return true;
    }
    let local_subdeps = &reachability.ctx.workspace.policy.local_subdeps;
    let key = (alias.to_string(), spec.to_string());
    if in_memory_parent_matches(local_subdeps, (parent_name, parent_version), &key) {
        return true;
    }
    lockfile_local_parent_declares_peer(reachability, (parent_name, parent_version), alias, spec)
}

fn in_memory_parent_matches(
    local_subdeps: &LocalSubdepAuthorizations,
    (parent_name, parent_version): (&str, &str),
    key: &(String, String),
) -> bool {
    let name_ver_map = lock_recoverable(&local_subdeps.pkg_name_ver);
    let untrusted = lock_recoverable(&local_subdeps.untrusted_local_peer_parents);
    let authorized = lock_recoverable(&local_subdeps.authorized_non_local_peer_specs);
    let local_by_pkg = lock_recoverable(&local_subdeps.local_pkg_peer_specs);
    let mut found_authorized = false;
    for (pkg_id, (name, ver)) in name_ver_map.iter() {
        if name != parent_name || ver != parent_version {
            continue;
        }
        if untrusted
            .get(pkg_id)
            .is_some_and(|map| map.contains_key(key))
        {
            return false;
        }
        if authorized
            .get(pkg_id)
            .is_some_and(|specs| specs.contains(key))
            || local_by_pkg
                .get(pkg_id)
                .is_some_and(|specs| specs.contains(key))
        {
            found_authorized = true;
        }
    }
    found_authorized
}

fn lockfile_local_parent_declares_peer(
    reachability: &HoistedPeerReachability<'_>,
    (parent_name, parent_version): (&str, &str),
    alias: &str,
    spec: &str,
) -> bool {
    let Some(lockfile) = reachability.ctx.workspace.wanted_lockfile() else {
        return false;
    };
    let Some(packages) = lockfile.packages.as_ref() else {
        return false;
    };
    let reachable_local = reachability.lockfile_reachable(true);
    packages
        .iter()
        .any(|(pkg_key, meta)| {
            pkg_key.name.to_string() == parent_name
                && meta.version.as_deref() == Some(parent_version)
                && reachable_local.contains(pkg_key)
                && meta.peer_dependencies
                    .as_ref()
                    .and_then(|peers| peers.get(alias))
                    .is_some_and(|declared| declared == spec)
        })
}

fn lockfile_non_local_peer_parent(
    reachability: &HoistedPeerReachability<'_>,
    alias: &str,
    spec: &str,
) -> Option<String> {
    let lockfile = reachability.ctx.workspace.wanted_lockfile()?;
    let packages = lockfile.packages.as_ref()?;
    let reachable = reachability.lockfile_reachable(false);
    packages
        .iter()
        .find_map(|(pkg_key, meta)| {
            if !reachable.contains(pkg_key)
                || matches!(pkg_key.suffix.version(), VersionPart::File(_))
            {
                return None;
            }
            lockfile_entry_untrusted_peer_parent(reachability.ctx, (pkg_key, meta), alias, spec)
        })
}

fn lockfile_entry_untrusted_peer_parent(
    ctx: &TreeCtx,
    (pkg_key, meta): (&PkgNameVerPeer, &PackageMetadata),
    alias: &str,
    spec: &str,
) -> Option<String> {
    let declares = meta.peer_dependencies
        .as_ref()
        .and_then(|peers| peers.get(alias))
        .is_some_and(|declared| declared == spec);
    if !declares {
        return None;
    }
    let key_str = pkg_key.to_string();
    let peer = (alias.to_string(), spec.to_string());
    if is_in_memory_authorized_peer(&ctx.workspace.policy.local_subdeps, &key_str, &peer) {
        return None;
    }
    let name_str = pkg_key.name.to_string();
    let ver_str = pkg_key.suffix
        .version_semver()
        .map(ToString::to_string)
        .or_else(|| meta.version.clone());
    (!override_authorizes_spec(ctx, Some((&name_str, ver_str.as_deref())), alias, spec)).then_some(
        key_str,
    )
}

fn is_in_memory_authorized_peer(
    local_subdeps: &LocalSubdepAuthorizations,
    pkg_id: &str,
    peer: &(String, String),
) -> bool {
    lock_recoverable(&local_subdeps.authorized_non_local_peer_specs)
        .get(pkg_id)
        .is_some_and(|specs| specs.contains(peer))
}
