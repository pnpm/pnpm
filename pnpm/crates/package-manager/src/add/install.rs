use super::{
    AddOptions, AddOwned,
    manifest::{PickedVersion, catalog_version_requests, merge_catalogs},
};
use crate::{
    ImporterUpdateSeedPolicy, Install, PolicyExcludes, ProjectMutation, UpdateSeedPolicy,
    included_direct_groups,
};
use node_semver::Version;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, PkgName, PkgVerPeer, ProjectSnapshot};
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
    let manifest_dir = manifest
        .path()
        .parent()
        .expect("manifest path always has a parent dir");
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
    add: AddOptions<'_>,
    owned: &AddOwned,
    manifest: &PackageManifest,
    selected: (&[pnpm_workspace::Project], &[usize]),
    picked_versions: &[Vec<PickedVersion>],
    catalogs_override: Option<Catalogs>,
    catalogs: &Catalogs,
) -> AddSeed {
    let (projects, selected_indices) = selected;
    let manifest_dir = manifest
        .path()
        .parent()
        .expect("manifest path always has a parent dir");
    let importer_root = add.config.lockfile_dir_for(manifest_dir);
    let mut seed_policies = BTreeMap::new();
    let mut preferred_versions_override = PreferredVersions::new();
    for (&index, picked) in selected_indices.iter().zip(picked_versions) {
        let (mut names, mut preferred) = catalog_version_requests(
            add.package_names,
            &projects[index].manifest,
            catalogs,
            add.lockfile.document,
            add.config,
            owned.save_catalog_name.as_deref(),
        );
        seed_picked_versions(
            picked,
            &LockedVersions::of(add.lockfile.document, add.config, &projects[index].root_dir),
            (&mut names, &mut preferred),
        );
        if !names.is_empty() {
            seed_policies.insert(
                pnpm_workspace::importer_id_from_root_dir(importer_root, &projects[index].root_dir),
                ImporterUpdateSeedPolicy::DropOnly(unversioned_targets(names)),
            );
        }
        for (name, selectors) in preferred {
            preferred_versions_override
                .entry(name)
                .or_default()
                .extend(selectors);
        }
    }
    AddSeed { seed_policies, preferred_versions_override, catalogs_override }
}
/// Prefer the version an explicit specifier picked, since the saved range can
/// admit newer ones, and withhold the pin the importer holds at another
/// version. A pick the fast lockfile update cannot move off needs no seed, so
/// the install can keep that fast path: the importer keeps its pin and its
/// specifier, or the lockfile holds no other version to retarget to.
pub(super) fn seed_picked_versions(
    picked_versions: &[PickedVersion],
    locked: &LockedVersions<'_>,
    seed: (&mut HashSet<String>, &mut PreferredVersions),
) {
    let (dropped_pins, preferred) = seed;
    for picked in picked_versions {
        let Some(lock) = locked.unsettled_pick(picked) else {
            continue;
        };
        crate::install_with_fresh_lockfile::prefer_requested_version(
            preferred,
            &picked.name,
            &picked.version,
        );
        if lock == PickLock::PinnedElsewhere {
            dropped_pins.insert(picked.alias.clone());
        }
    }
}
/// How the importer locks a pick the install has to move to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickLock {
    Unpinned,
    PinnedElsewhere,
}
/// What the wanted lockfile holds for the importer an add rewrites.
pub(super) struct LockedVersions<'l> {
    lockfile: Option<&'l Lockfile>,
    importer: Option<&'l ProjectSnapshot>,
}
impl<'l> LockedVersions<'l> {
    pub(super) fn of(
        lockfile: Option<&'l Lockfile>,
        config: &Config,
        manifest_dir: &std::path::Path,
    ) -> Self {
        let importer_id = pnpm_workspace::importer_id_from_root_dir(
            config.lockfile_dir_for(manifest_dir),
            manifest_dir,
        );
        let importer = lockfile.and_then(|lockfile| lockfile.importers.get(&importer_id));
        Self { lockfile, importer }
    }

    /// `None` when the fast lockfile update already lands on `picked`.
    fn unsettled_pick(&self, picked: &PickedVersion) -> Option<PickLock> {
        let PickedVersion { alias, name, version, specifier } = picked;
        let (Ok(alias), Ok(name), Ok(version)) =
            (alias.parse::<PkgName>(), name.parse::<PkgName>(), version.parse::<Version>())
        else {
            return None;
        };
        let direct = self.direct(&alias);
        let pinned_elsewhere = direct.is_some_and(|(locked, _)| {
            !locked.is_some_and(|locked| same_build(locked, &version))
        });
        let settled = match direct {
            Some(_) if pinned_elsewhere => false,
            Some((_, locked_specifier)) if locked_specifier == specifier => true,
            _ => self.only_version_of(&name, &version),
        };
        match (settled, pinned_elsewhere) {
            (true, _) => None,
            (false, false) => Some(PickLock::Unpinned),
            (false, true) => Some(PickLock::PinnedElsewhere),
        }
    }

    /// The version and specifier the importer locks `name` at as a direct
    /// dependency.
    fn direct(&self, name: &PkgName) -> Option<(Option<&'l Version>, &'l str)> {
        let (_, dependency) = self.importer?
            .dependencies_by_groups(crate::DIRECT_GROUPS)
            .find(|(alias, _)| *alias == name)?;
        let version = dependency.version.as_regular().and_then(PkgVerPeer::version_semver);
        Some((version, dependency.specifier.as_str()))
    }

    /// Whether `version` is the one version of `name` the lockfile holds, which
    /// is then what the fast path links a new direct dependency to.
    fn only_version_of(&self, name: &PkgName, version: &Version) -> bool {
        let mut versions = self.lockfile
            .and_then(|lockfile| lockfile.packages.as_ref())
            .into_iter()
            .flatten()
            .filter(|(key, _)| key.name == *name)
            .map(|(key, _)| key.suffix.version_semver())
            .peekable();
        versions.peek().is_some()
            && versions.all(|locked| locked.is_some_and(|locked| same_build(locked, version)))
    }
}
/// `Version`'s own equality ignores build metadata, which names a different
/// package here.
pub(super) fn same_build(locked: &Version, picked: &Version) -> bool {
    locked == picked && locked.build == picked.build
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
/// include filter: unless `included_groups` narrows it, and like
/// `remove`, the re-resolve walks every dependency group so the other
/// groups' entries stay in the lockfile, the virtual store, and
/// `node_modules`.
/// `None` defers to `config.prefer_frozen_lockfile`, which is
/// what lets the fast lockfile update absorb the manifest edit
/// [`prepare_manifest`](crate::add::manifest::prepare_manifest) just made. It only absorbs an addition the
/// lockfile already holds a satisfying version for; anything else
/// fails the freshness gate and reaches the resolver.
/// A `catalog:` dependency's manifest specifier doesn't change when the version
/// behind it does, so the freshness gate would hold and the install would never
/// reach the resolver. Nor would it absorb a version an explicit specifier
/// picked below one the lockfile already holds.
pub(super) fn add_install<'i>(
    add: AddOptions<'i>,
    owned: AddOwned,
    manifest: &'i PackageManifest,
    seed: AddSeed,
) -> Install<'i, Vec<DependencyGroup>> {
    let named_a_version = !seed.seed_policies.is_empty();
    let reresolves = named_a_version || !seed.preferred_versions_override.is_empty();
    let included_groups = owned.included_groups.unwrap_or_else(|| {
        included_direct_groups(add.config.optional).collect()
    });
    let mut install = Install::new(
        owned.tarball_mem_cache,
        add.resolved_packages,
        (add.http_client, owned.http_client_arc),
        add.config,
        manifest,
        add.lockfile.source,
        included_groups,
    );
    install.lockfile_policy.prefer_frozen = reresolves.then_some(false);
    install.lockfile_policy.excludes = PolicyExcludes::Persist;
    install.execution.mutation = ProjectMutation::InstallSome;
    install.execution.installs_only = false;
    install.execution.lockfile_only = add.lockfile_only;
    install.resolution.update_seed_policy = if named_a_version {
        UpdateSeedPolicy::ByImporter {
            policies: seed.seed_policies,
            max_depth: UpdateDepth::new(0),
        }
    } else {
        UpdateSeedPolicy::KeepAll
    };
    install.resolution.preferred_versions_override = Some(seed.preferred_versions_override.into());
    install.context.emit_initial_manifest = false;
    install.context.lockfile_path = add.lockfile.path;
    install.projects.supported_architectures = owned.supported_architectures;
    install.projects.catalogs_override = seed.catalogs_override;
    install
}
/// Update targets that no selector scoped to a version line: a `catalog:`
/// re-resolution moves whatever version the catalog entry now names.
pub(super) fn unversioned_targets(names: HashSet<String>) -> UpdateTargets {
    names
        .into_iter()
        .map(|name| (name, None))
        .collect()
}
