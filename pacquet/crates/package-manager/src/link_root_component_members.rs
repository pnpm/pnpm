use crate::{SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout, symlink_package};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{
    ImporterDepVersion, PackageKey, PkgName, ProjectSnapshot, ResolvedDependencySpec, VersionPart,
};
use pacquet_package_manifest::DependencyGroup;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// The `installConfig.hoistingLimits` value that marks a Bit "root
/// component" importer. Bit stamps it on the per-root importer
/// manifests it writes under `node_modules/.bit_roots/<id>`; no
/// non-Bit install produces it, so it doubles as the gate for the
/// cross-linking below.
pub const HOISTING_LIMITS_WORKSPACES: &str = "workspaces";

/// Cross-link each Bit root component's injected members into one
/// another's virtual-store slot so they are mutually reachable.
///
/// ## The problem this solves
/// Bit's `bit install --root-components` writes one importer per root
/// under `node_modules/.bit_roots/<id>` whose manifest injects the
/// root's sibling components as `workspace:*` deps and carries
/// `installConfig: { hoistingLimits: "workspaces" }`. Under
/// `nodeLinker: isolated` those injected members are materialized as
/// `file:` deps, each landing in its own global-virtual-store slot at
/// `<store>/links/<name>/<ver>/<hash>/node_modules/<name>`. A member's
/// slot `node_modules/` only holds that member's own lockfile deps —
/// it does not hold the sibling members. So a `realpath`-based
/// resolution (Node's default, `preserveSymlinks: false`) that lands
/// inside a member's slot and walks up `node_modules/` never reaches a
/// sibling, and `require('@scope/sibling')` throws `MODULE_NOT_FOUND`
/// even though the root importer wired the sibling in.
///
/// The generally-correct fix is a per-root local virtual store; the
/// pragmatic fix here is to symlink every one of a root's injected
/// members into every other member's slot `node_modules/`, so the
/// upward walk from any member finds its siblings.
///
/// ## Why this is safe
/// It fires only for importers whose manifest declares
/// `installConfig.hoistingLimits: "workspaces"` (`root_component_importers`),
/// a shape no non-Bit install produces — so ordinary installs are
/// untouched. It is purely additive: an entry already present in a
/// member's slot (a real dependency the member resolves for itself) is
/// never overwritten. And because each root's peer context is folded
/// into its members' global-virtual-store hashes, two different roots
/// get distinct member slots; cross-linking one root's members can
/// never leak a wrongly-resolved sibling into another root's chain.
pub fn link_root_component_members(
    layout: &VirtualStoreLayout,
    importers: &HashMap<String, ProjectSnapshot>,
    root_component_importers: &HashSet<String>,
    dependency_groups: &[DependencyGroup],
    skipped: &SkippedSnapshots,
) -> Result<(), LinkRootComponentMembersError> {
    // No root components → nothing to do. Keeps every ordinary install
    // out of the per-importer scan below.
    if root_component_importers.is_empty() {
        return Ok(());
    }

    for (importer_id, importer) in importers {
        if !root_component_importers.contains(importer_id.as_str()) {
            continue;
        }
        let members = collect_injected_members(layout, importer, dependency_groups, skipped);
        cross_link_members(&members)?;
    }

    Ok(())
}

/// One injected `file:` member of a root component, resolved to the
/// on-disk paths the cross-link pass needs.
struct Member {
    /// `@scope/name` — the name siblings import this member as, and the
    /// directory name under `node_modules/`.
    name: String,
    /// `<slot>/node_modules` — where this member's dependency symlinks
    /// live, and where sibling symlinks get added.
    slot_modules_dir: PathBuf,
    /// `<slot>/node_modules/<name>` — the member's own package
    /// directory, the target a sibling's symlink points at.
    package_dir: PathBuf,
}

/// Collect a root importer's injected `file:` members. Only injected
/// members participate: those are the per-root component copies that
/// each get an isolated global-virtual-store slot. Registry deps
/// (`react` etc.) resolve correctly within each member's slot already,
/// and `link:` deps point straight at a workspace directory whose own
/// `node_modules/` is reachable — neither needs cross-linking.
///
/// An injected member surfaces at the importer level as either an
/// `Alias` whose resolved version is a `file:` spec (the scoped shape
/// `@scope/name@file:<path>(peers)`, which parses to `Alias` because it
/// leads with the package name) or a bare `File`. Both are matched by
/// keying off the resolved snapshot key's `file:` version.
fn collect_injected_members(
    layout: &VirtualStoreLayout,
    importer: &ProjectSnapshot,
    dependency_groups: &[DependencyGroup],
    skipped: &SkippedSnapshots,
) -> Vec<Member> {
    let mut seen: HashSet<&PkgName> = HashSet::new();
    let mut members = Vec::new();
    for group in dependency_groups.iter().copied() {
        if matches!(group, DependencyGroup::Peer) {
            continue;
        }
        let Some(deps) = importer.get_map_by_group(group) else { continue };
        for (name, spec) in deps {
            let Some((dir_name, key)) = injected_member_key(name, spec) else { continue };
            // First-wins across groups, matching the symlink stage's
            // dedup so a member listed in more than one group is only
            // materialized once.
            if !seen.insert(name) {
                continue;
            }
            // A member the installability pass skipped has no slot on
            // disk, so there is nothing to link (and nothing that would
            // link to it).
            if skipped.contains(&key) {
                continue;
            }
            let slot_modules_dir = layout.slot_dir(&key).join("node_modules");
            let package_dir = slot_modules_dir.join(&dir_name);
            members.push(Member { name: dir_name, slot_modules_dir, package_dir });
        }
    }
    members
}

/// `Some((dir_name, snapshot_key))` when `spec` resolves to an injected
/// `file:` virtual-store slot; `None` for registry, `link:`, and
/// non-injected deps. `dir_name` is the package name siblings resolve
/// this member as (the `Alias`'s real name, or the importer-map key for
/// a bare `File`).
fn injected_member_key(
    name: &PkgName,
    spec: &ResolvedDependencySpec,
) -> Option<(String, PackageKey)> {
    match &spec.version {
        // Bare `file:<path>` — the resolved key reuses the importer-map
        // key as both the snapshot name and the `node_modules/` dir.
        ImporterDepVersion::File(_) => {
            spec.version.resolved_key(name).map(|key| (name.to_string(), key))
        }
        // `@scope/name@file:<path>(peers)` parses to `Alias`; it is an
        // injected member only when its version is a `file:` spec. The
        // alias's own name is the `node_modules/` dir siblings import.
        ImporterDepVersion::Alias(alias)
            if matches!(alias.suffix.version(), VersionPart::File(_)) =>
        {
            Some((alias.name.to_string(), alias.clone()))
        }
        ImporterDepVersion::Regular(_)
        | ImporterDepVersion::Alias(_)
        | ImporterDepVersion::Link(_) => None,
    }
}

/// Symlink every member into every other member's slot `node_modules/`.
fn cross_link_members(members: &[Member]) -> Result<(), LinkRootComponentMembersError> {
    for host in members {
        for sibling in members {
            // A member's own package directory already exists in its
            // slot; never link a member into itself.
            if host.name == sibling.name {
                continue;
            }
            let symlink_path = host.slot_modules_dir.join(&sibling.name);
            // Purely additive. `symlink_metadata` doesn't follow the
            // final component, so an existing symlink — dangling or not
            // — counts as present and is left untouched; only a genuinely
            // missing sibling is filled in. This never clobbers a real
            // dependency a member resolves for itself.
            if std::fs::symlink_metadata(&symlink_path).is_ok() {
                continue;
            }
            symlink_package(&sibling.package_dir, &symlink_path).map_err(|source| {
                LinkRootComponentMembersError::Symlink {
                    member: host.name.clone(),
                    sibling: sibling.name.clone(),
                    source,
                }
            })?;
        }
    }
    Ok(())
}

/// Error type of [`link_root_component_members`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkRootComponentMembersError {
    /// A sibling-into-member symlink failed (e.g. permission denied,
    /// disk full, an existing non-symlink file squatting the path).
    #[display("Failed to link root-component sibling {sibling:?} into member {member:?}: {source}")]
    #[diagnostic(code(pacquet_package_manager::root_component_symlink_failed))]
    Symlink {
        member: String,
        sibling: String,
        #[error(source)]
        source: SymlinkPackageError,
    },
}

#[cfg(test)]
mod tests;
