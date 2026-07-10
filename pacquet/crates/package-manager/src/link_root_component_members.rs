use crate::{SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout, symlink_package};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{
    ImporterDepVersion, PackageKey, PkgName, ProjectSnapshot, ResolvedDependencySpec, VersionPart,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// The `installConfig.hoistingLimits` value that marks a Bit "root
/// component" importer. Bit stamps it on the per-root importer
/// manifests it writes under `node_modules/.bit_roots/<id>`; no
/// non-Bit install produces it, so it doubles as the gate for the
/// sibling linking below.
pub const HOISTING_LIMITS_WORKSPACES: &str = "workspaces";

/// Give each Bit root component's injected members the sibling
/// dependencies they declare, so a member's slot `node_modules/` is
/// self-contained enough for a `realpath`-based resolution walk.
///
/// ## The problem this solves
/// Bit's `bit install --root-components` writes one importer per root
/// under `node_modules/.bit_roots/<id>` whose manifest injects the
/// root's sibling components and carries
/// `installConfig: { hoistingLimits: "workspaces" }`. Under
/// `nodeLinker: isolated` those injected members are materialized as
/// `file:` deps, each landing in its own global-virtual-store slot at
/// `<store>/links/<name>/<ver>/<hash>/node_modules/<name>`. Because Bit
/// installs with `excludeLinksFromLockfile` + `dedupeInjectedDeps`, a
/// member's edges to its sibling members don't survive into the slot,
/// so a member's slot `node_modules/` ends up missing the siblings it
/// depends on. A `realpath`-based resolution (Node's default,
/// `preserveSymlinks: false`) that lands inside a member's slot and
/// walks up `node_modules/` then can't reach a declared sibling, and
/// `require('@scope/sibling')` throws `MODULE_NOT_FOUND`.
///
/// This restores exactly the edges the isolated linker creates for an
/// ordinary dependency: for each member it symlinks the siblings that
/// member *declares in its own manifest* into its slot `node_modules/`.
/// It mirrors pnpm — each package's slot holds only its own declared
/// children, and a transitive sibling resolves through the chain
/// (`comp3 → comp2 → comp1`) rather than an all-to-all clique.
///
/// ## Why this is safe
/// It fires only for importers whose manifest declares
/// `installConfig.hoistingLimits: "workspaces"` (`root_component_importers`),
/// a shape no non-Bit install produces — so ordinary installs are
/// untouched. It is purely additive: an entry already present in a
/// member's slot (a dependency the member resolves for itself) is never
/// overwritten. And within one root the member names are unique (each
/// root's peer context yields distinct slots), so a declared sibling
/// name resolves to exactly that root's copy — linking one root's
/// members can never leak a sibling into another root's chain.
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
        link_declared_siblings(&members)?;
    }

    Ok(())
}

/// One injected `file:` member of a root component, resolved to the
/// on-disk paths the sibling-linking pass needs.
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
/// `node_modules/` is reachable — neither needs sibling linking.
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

/// Symlink each member's sibling dependencies into its slot
/// `node_modules/`.
///
/// A member's manifest is the source of truth for which siblings it
/// depends on — it survives `excludeLinksFromLockfile` /
/// `dedupeInjectedDeps`, where the lockfile edges do not. Only the
/// `dependencies` / `optionalDependencies` / `peerDependencies` a
/// member declares are linked, and only when the named package is
/// itself a member of this same root (its peer-resolved slot), matching
/// the isolated linker's per-package child linking.
///
/// A Bit workspace component carries no `package.json` in its source
/// directory (its manifest lives only in memory on the Bit side, and
/// Bit's read-package hooks strip sibling edges from it before it ever
/// reaches the engine), so the materialized slot has no manifest to
/// read. For such a member the declared-edge information is simply
/// unavailable — fall back to linking **every** other member of this
/// root into its slot. The clique is safe: member names are unique
/// within one root's peer context, the links are additive (an entry the
/// member already resolves for itself is never overwritten), and a
/// member only ever `require`s what it actually depends on, so surplus
/// links are inert. A manifest that exists but cannot be parsed is
/// still a hard error.
fn link_declared_siblings(members: &[Member]) -> Result<(), LinkRootComponentMembersError> {
    // Within one root importer each member name is unique (one peer
    // context per root), so a declared name identifies at most one
    // sibling slot.
    let by_name: HashMap<&str, &Member> =
        members.iter().map(|member| (member.name.as_str(), member)).collect();

    for host in members {
        let manifest_path = host.package_dir.join("package.json");
        let manifest = match PackageManifest::from_path(manifest_path.clone()) {
            Ok(manifest) => Some(manifest),
            // No manifest on disk — the Bit-workspace shape. Link the
            // whole root clique instead (see the doc comment above).
            Err(PackageManifestError::NoImporterManifestFound(_)) => None,
            Err(source) => {
                return Err(LinkRootComponentMembersError::ReadManifest {
                    member: host.name.clone(),
                    path: manifest_path,
                    source,
                });
            }
        };
        let siblings: Vec<&Member> = match &manifest {
            Some(manifest) => manifest
                .dependencies([
                    DependencyGroup::Prod,
                    DependencyGroup::Optional,
                    DependencyGroup::Peer,
                ])
                // Only siblings that belong to this root; a member's own
                // package dir already lives in its slot.
                .filter_map(|(dep_name, _)| by_name.get(dep_name).copied())
                .collect(),
            None => members.iter().collect(),
        };
        for sibling in siblings {
            if sibling.name == host.name {
                continue;
            }
            let symlink_path = host.slot_modules_dir.join(&sibling.name);
            // Additive. `symlink_metadata` doesn't follow the final
            // component, so an existing symlink — dangling or not —
            // counts as present and is left untouched; only a genuinely
            // missing sibling is filled in. This never clobbers a
            // dependency the member already resolves for itself.
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
    /// An injected member's manifest could not be read to learn which
    /// siblings it declares.
    #[display("Failed to read manifest of root-component member {member:?} at {path:?}: {source}")]
    #[diagnostic(code(pacquet_package_manager::root_component_manifest_read_failed))]
    ReadManifest {
        member: String,
        path: PathBuf,
        #[error(source)]
        source: PackageManifestError,
    },

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
