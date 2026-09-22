//! What the resolver reads off an *importer's* manifest to seed a walk:
//! which of its direct dependencies are optional or injected, and the
//! wanted specs the walk starts from. The package side of the same job —
//! what the walk reads off each resolved package — lives in
//! [`super::manifest`].

use std::path::Path;

use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
};
use pnpm_resolving_resolver_base::is_acceptable_peer_spec;
use rustc_hash::{
    FxHashMap as HashMap,
    FxHashSet as HashSet,
};
use serde_json::Value;

use super::{
    ResolveDependencyTreeError,
    WantedSpec,
    catalogs::catalog_anchor,
    dependency_meta_is_injected,
    resolve_catalog_specifiers,
};

/// Collect the names of the importer manifest's `optionalDependencies`
/// entries so the walker can tag each direct dep with the right
/// `wanted.optional` flag. `optionalDependencies` wins over the other
/// groups when an alias appears in more than one, so the
/// `ResolvedPackage.optional` propagation starts from the right
/// per-direct-dep value.
pub(super) fn importer_optional_dependency_names(manifest: &PackageManifest) -> HashSet<String> {
    manifest
        .dependencies([DependencyGroup::Optional])
        .map(|(name, _)| name.to_string())
        .collect()
}

/// Collect the names of the importer manifest's `dependenciesMeta` entries
/// whose `injected` flag is `true`. This per-alias `injected` opt-in
/// flips a workspace dep onto the hard-linked `file:` path even when the
/// global `injectWorkspacePackages` is off.
pub(super) fn importer_injected_dependency_names(manifest: &PackageManifest) -> HashSet<String> {
    injected_dependency_names(manifest.value())
}

fn injected_dependency_names(manifest: &Value) -> HashSet<String> {
    let Some(meta) = manifest.get("dependenciesMeta").and_then(Value::as_object) else {
        return HashSet::default();
    };
    meta.iter()
        .filter(|(_, entry)| dependency_meta_is_injected(entry))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Build the importer's direct-dependency wanted specs: the manifest's
/// `dependencies` (plus, when `auto_install_peers`, its own
/// `peerDependencies`) tagged with the right `optional` / `injected`
/// flags and with `catalog:` specifiers resolved.
///
/// `workspace_dir` is where `pnpm-workspace.yaml` sits, so a `file:` /
/// `link:` catalog entry lands on the path the manifest's own directory
/// would have written.
///
/// An alias declared in several groups yields one spec, merged by
/// spreading the groups in order: `peerDependencies` first (when
/// `auto_install_peers`), then `devDependencies` < `dependencies` <
/// `optionalDependencies`, a later group's range replacing an earlier
/// one — matching `filterDependenciesByType` in
/// `@pnpm/pkg-manifest.utils` (`{...dev, ...prod, ...optional}`), so a
/// regular dep wins over a devDependency of the same alias, and either
/// wins over its peer range.
///
/// Shared by [`fn@crate::resolve_importer`] (which walks them) and the
/// `time-based` cutoff pre-pass in [`fn@crate::resolve_workspace`]
/// (which only needs the resolved direct-dep publish dates), so both
/// see the identical direct-dep set — the importer-dep computation runs
/// once before resolving an importer's deps.
///
/// Fails with
/// [`InvalidPeerDependencySpecification`](ResolveDependencyTreeError::InvalidPeerDependencySpecification)
/// when the manifest declares a `peerDependencies` value that is not an
/// acceptable peer spec, whatever `auto_install_peers` and
/// `dependency_groups` select. Only a resolving install reaches this; an
/// install served from an up-to-date lockfile validates nothing, as on
/// pnpm v11.
pub(crate) fn importer_direct_wanted_specs<DependencyGroupList>(
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    auto_install_peers: bool,
    catalogs: &Catalogs,
    workspace_dir: Option<&Path>,
) -> Result<Vec<WantedSpec>, ResolveDependencyTreeError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    validate_peer_dependencies(manifest)?;
    let included: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
    let mut groups: Vec<DependencyGroup> = Vec::new();
    if auto_install_peers || included.contains(&DependencyGroup::Peer) {
        groups.push(DependencyGroup::Peer);
    }
    groups.extend(
        [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional]
            .into_iter()
            .filter(|group| included.contains(group)),
    );
    let optional_names = importer_optional_dependency_names(manifest);
    let injected_names = importer_injected_dependency_names(manifest);
    let mut order: Vec<&str> = Vec::new();
    let mut ranges: HashMap<&str, &str> = HashMap::default();
    for (name, range) in manifest.dependencies(groups) {
        if !crate::is_valid_dependency_alias(name) {
            return Err(ResolveDependencyTreeError::InvalidDependencyName {
                parent: "The current package".to_string(),
                alias: name.to_string(),
            });
        }
        if ranges.insert(name, range).is_none() {
            order.push(name);
        }
    }
    let wanted: Vec<WantedSpec> = order
        .into_iter()
        .map(|name| {
            (
                name.to_string(),
                ranges[name].to_string(),
                optional_names.contains(name),
                injected_names.contains(name),
            )
        })
        .collect();
    let consumer_dir = manifest.path().parent();
    resolve_catalog_specifiers(wanted, catalogs, catalog_anchor(workspace_dir, consumer_dir))
}

/// Reject a `peerDependencies` value that is neither a peer range nor a
/// scheme-carrying specifier, so a `<name>@<version>` typo fails the install
/// instead of resolving as a project-relative directory.
fn validate_peer_dependencies(
    manifest: &PackageManifest,
) -> Result<(), ResolveDependencyTreeError> {
    for (dep_name, specifier) in manifest.dependencies([DependencyGroup::Peer]) {
        if !is_acceptable_peer_spec(specifier) {
            return Err(ResolveDependencyTreeError::InvalidPeerDependencySpecification {
                dep_name: dep_name.to_string(),
                project_id: manifest_project_id(manifest),
                specifier: specifier.to_string(),
            });
        }
    }
    Ok(())
}

/// How an importer is named in a diagnostic: its manifest's `name`, falling
/// back to the directory holding the manifest for the unnamed root manifest a
/// workspace usually has.
fn manifest_project_id(manifest: &PackageManifest) -> String {
    if let Some(name) = manifest
        .value()
        .get("name")
        .and_then(Value::as_str)
    {
        return name.to_string();
    }
    manifest
        .path()
        .parent()
        .unwrap_or_else(|| manifest.path())
        .display()
        .to_string()
}
