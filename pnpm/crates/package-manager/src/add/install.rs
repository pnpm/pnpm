use super::{
    AddOwned, AddView,
    manifest::{catalog_version_requests, merge_catalogs},
};
use crate::{
    ImporterUpdateSeedPolicy, Install, PolicyExcludes, ProjectMutation, UpdateSeedPolicy,
    included_direct_groups,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::MaybeLazyLockfile;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_deps_resolver::{UpdateDepth, UpdateTargets};
use pnpm_resolving_resolver_base::PreferredVersions;
use std::collections::{BTreeMap, HashSet};

/// Scoped to this project's importer: a sibling that declares the same package
/// keeps its pin, so its resolution stands.
pub(super) fn project_seed_policy(
    config: &Config,
    manifest: &PackageManifest,
    dropped_pins: HashSet<String>,
) -> BTreeMap<String, ImporterUpdateSeedPolicy> {
    if dropped_pins.is_empty() {
        return BTreeMap::new();
    }
    let manifest_dir = manifest.path().parent().expect("manifest path always has a parent dir");
    BTreeMap::from([(
        pnpm_workspace::importer_id_from_root_dir(
            config.lockfile_dir_for(manifest_dir),
            manifest_dir,
        ),
        ImporterUpdateSeedPolicy::DropOnly(unversioned_targets(dropped_pins)),
    )])
}
/// The catalogs the install resolves against when the add changed any,
/// and none when the workspace manifest already says it all.
pub(super) fn merged_catalogs_override(
    mut catalogs: Catalogs,
    updated_catalogs: &Catalogs,
) -> Option<Catalogs> {
    if updated_catalogs.is_empty() {
        return None;
    }
    merge_catalogs(&mut catalogs, updated_catalogs);
    Some(catalogs)
}
/// Scoped per importer: a project that wasn't selected keeps its pins, so its
/// resolutions stand even when it declares the same package directly.
pub(super) fn selected_add_seed(
    add: AddView<'_>,
    owned: &AddOwned,
    manifest: &PackageManifest,
    selected: (&[pnpm_workspace::Project], &[usize]),
    catalogs_override: Option<Catalogs>,
    catalogs: &Catalogs,
) -> AddSeed {
    let (projects, selected_indices) = selected;
    let manifest_dir = manifest.path().parent().expect("manifest path always has a parent dir");
    let importer_root = add.config.lockfile_dir_for(manifest_dir);
    let mut seed_policies = BTreeMap::new();
    let mut preferred_versions_override = PreferredVersions::new();
    for &index in selected_indices {
        let (names, preferred) = catalog_version_requests(
            add.package_names,
            &projects[index].manifest,
            catalogs,
            add.lockfile,
            add.config,
            owned.save_catalog_name.as_deref(),
        );
        if names.is_empty() {
            continue;
        }
        seed_policies.insert(
            pnpm_workspace::importer_id_from_root_dir(importer_root, &projects[index].root_dir),
            ImporterUpdateSeedPolicy::DropOnly(unversioned_targets(names)),
        );
        for (name, selectors) in preferred {
            preferred_versions_override.entry(name).or_default().extend(selectors);
        }
    }
    AddSeed { seed_policies, preferred_versions_override, catalogs_override }
}
/// What the install resolves from: the importers whose catalog pins are
/// withheld, the versions the add named, and the catalogs as it rewrote
/// them.
pub(super) struct AddSeed {
    pub(super) seed_policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
    pub(super) preferred_versions_override: PreferredVersions,
    pub(super) catalogs_override: Option<Catalogs>,
}
/// `dependency_groups` names the manifest group the new
/// package is saved into ([`prepare_manifest`](crate::add::manifest::prepare_manifest)), not an
/// include filter: like `remove`, the re-resolve walks every
/// dependency group so the other groups' entries stay in the
/// lockfile, the virtual store, and `node_modules`.
/// `None` defers to `config.prefer_frozen_lockfile`, which is
/// what lets the fast lockfile update absorb the manifest edit
/// [`prepare_manifest`](crate::add::manifest::prepare_manifest) just made. It only absorbs an addition the
/// lockfile already holds a satisfying version for; anything else
/// fails the freshness gate and reaches the resolver.
/// A `catalog:` dependency's manifest specifier doesn't change when the version
/// behind it does, so the freshness gate would hold and the install would never
/// reach the resolver.
pub(super) fn add_install<'i>(
    add: AddView<'i>,
    owned: AddOwned,
    manifest: &'i PackageManifest,
    seed: AddSeed,
) -> Install<'i, impl Iterator<Item = DependencyGroup>> {
    let named_a_version = !seed.seed_policies.is_empty();
    Install {
        emit_initial_manifest: false,
        lockfile_path: add.lockfile_path,
        prefer_frozen_lockfile: named_a_version.then_some(false),
        mutation: ProjectMutation::InstallSome,
        installs_only: false,
        supported_architectures: owned.supported_architectures,
        lockfile_only: add.lockfile_only,
        policy_excludes: PolicyExcludes::Persist,
        // `add` keeps every lockfile pin; the freshly-added range
        // is the only thing that re-resolves. `update`'s bump is a
        // separate operation.
        update_seed_policy: if named_a_version {
            UpdateSeedPolicy::ByImporter {
                policies: seed.seed_policies,
                // A catalog entry governs direct dependencies, so the pin is
                // withheld there and transitive occurrences of the same package
                // keep theirs.
                max_depth: UpdateDepth::new(0),
            }
        } else {
            UpdateSeedPolicy::KeepAll
        },
        preferred_versions_override: Some(seed.preferred_versions_override),
        catalogs_override: seed.catalogs_override,
        ..Install::new(
            owned.tarball_mem_cache,
            add.resolved_packages,
            (add.http_client, owned.http_client_arc),
            add.config,
            manifest,
            MaybeLazyLockfile::Loaded(add.lockfile),
            included_direct_groups(add.config.optional),
        )
    }
}
/// The lockfile pins to withhold, and the preferences to layer on the seed,
/// for a version an `add` named that its catalog entry resolves past.
///
/// A cataloged dependency writes `catalog:` to the manifest and keeps its
/// version in the catalog entry, so a version named on the command line has
/// nowhere else to land: without this the entry's recorded resolution is
/// reused and the request is dropped in silence. Every other `add` — a
/// dependency that isn't cataloged, a catalog entry that already resolves to
/// the wanted version, one the wanted version falls outside of — is left
/// alone, so an add that needs no resolution still skips it.
/// Update targets that no selector scoped to a version line: a `catalog:`
/// re-resolution moves whatever version the catalog entry now names.
pub(super) fn unversioned_targets(names: HashSet<String>) -> UpdateTargets {
    names.into_iter().map(|name| (name, None)).collect()
}
