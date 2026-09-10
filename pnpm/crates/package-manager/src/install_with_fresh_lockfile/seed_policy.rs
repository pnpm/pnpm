use pnpm_resolving_deps_resolver::{UpdateDepth, UpdateTargets};
use std::collections::BTreeMap;

/// Which lockfile-pinned `(name, version)` pairs to *withhold* from the
/// preferred-versions tie-break seed [`InstallWithFreshLockfile`](crate::InstallWithFreshLockfile) builds
/// via `get_preferred_versions_from_lockfile_and_manifests`.
///
/// A name whose pin is withheld no longer carries its previously-locked
/// version at the existing-version weight, so the resolver falls back to
/// picking the highest version satisfying the manifest range — the
/// compatible re-resolution `pacquet update` performs. This is the
/// `update: 'compatible'` resolver mode, which ignores the lockfile
/// version for the dependency being updated.
///
/// `KeepAll` is the install/add default (every pin seeds the table, so
/// unrelated entries keep their resolutions on a rewrite).
///
/// Every withholding variant carries the update's `--depth` ceiling,
/// which bounds how deep the re-resolution reaches: a node past it keeps
/// its locked resolution even when its name is a target. See
/// [`UpdateDepth`].
#[derive(Debug, Default, Clone)]
pub enum UpdateSeedPolicy {
    /// Seed every lockfile pin. `pacquet install` / `pacquet add`.
    #[default]
    KeepAll,
    /// Seed every lockfile pin but re-resolve every dependency edge.
    /// `pacquet dedupe` uses this to preserve valid pins while rebuilding
    /// the graph around the fewest compatible versions.
    KeepAllResolveAll,
    /// Preserve locked versions while regenerating all derived lockfile data.
    FixLockfile,
    /// Re-resolve every registry edge at its locked version using fresh
    /// metadata. `pacquet update --patches` uses this to pick the registry's
    /// current revision without allowing semver movement.
    RefreshRevisions,
    /// Withhold every lockfile pin. `pacquet update` with no package
    /// selectors — the whole graph re-resolves to highest-in-range.
    DropAll {
        max_depth: UpdateDepth,
    },
    /// Withhold only the update targets' pins. `pacquet update <pattern>`
    /// — a matched name re-resolves while everything else keeps its pin,
    /// and a selector that pinned an exact version narrows the target to
    /// that version line. Keyed by package name (scope included); see
    /// [`UpdateTargets`].
    DropOnly {
        targets: UpdateTargets,
        max_depth: UpdateDepth,
    },
    ByImporter {
        policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
        max_depth: UpdateDepth,
    },
}
/// Record `version` as the preferred one for `name`, outranking the pin the
/// lockfile seeds.
///
/// A version named on the command line has to reach the lockfile even when
/// the specifier written to the manifest doesn't carry it — a `catalog:`
/// entry keeps the version in the catalog, so without this the entry's
/// recorded resolution is reused and the request is dropped silently.
pub(crate) fn prefer_requested_version(
    preferred: &mut pnpm_resolving_resolver_base::PreferredVersions,
    name: &str,
    version: &str,
) {
    use pnpm_resolving_resolver_base::{
        EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry, VersionSelectorType,
        VersionSelectorWithWeight,
    };

    if node_semver::Version::parse(version).is_err() {
        return;
    }
    preferred.entry(name.to_string()).or_default().insert(
        version.to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT + 1,
        }),
    );
}
impl UpdateSeedPolicy {
    /// Withhold every pin at every depth — the re-resolve `pacquet
    /// dedupe` / `pacquet import` and the napi install perform, none of
    /// which expose a `--depth`.
    #[must_use]
    pub fn drop_all() -> Self {
        UpdateSeedPolicy::DropAll { max_depth: UpdateDepth::UNLIMITED }
    }

    pub(super) fn max_depth(&self) -> UpdateDepth {
        match self {
            UpdateSeedPolicy::KeepAll
            | UpdateSeedPolicy::KeepAllResolveAll
            | UpdateSeedPolicy::FixLockfile
            | UpdateSeedPolicy::RefreshRevisions => UpdateDepth::UNLIMITED,
            UpdateSeedPolicy::DropAll { max_depth }
            | UpdateSeedPolicy::DropOnly { max_depth, .. }
            | UpdateSeedPolicy::ByImporter { max_depth, .. } => *max_depth,
        }
    }
}
#[derive(Debug, Clone)]
pub enum ImporterUpdateSeedPolicy {
    DropAll,
    DropOnly(UpdateTargets),
}
pub(super) fn update_reuse_scopes(
    policy: &UpdateSeedPolicy,
) -> (
    pnpm_resolving_deps_resolver::UpdateReuseScope,
    BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
) {
    use pnpm_resolving_deps_resolver::UpdateReuseScope;

    match policy {
        UpdateSeedPolicy::KeepAll => (UpdateReuseScope::All, BTreeMap::new()),
        UpdateSeedPolicy::KeepAllResolveAll
        | UpdateSeedPolicy::FixLockfile
        | UpdateSeedPolicy::RefreshRevisions => (UpdateReuseScope::None, BTreeMap::new()),
        UpdateSeedPolicy::DropAll { .. } => (UpdateReuseScope::None, BTreeMap::new()),
        UpdateSeedPolicy::DropOnly { targets, .. } => {
            (UpdateReuseScope::Except(targets.clone()), BTreeMap::new())
        }
        UpdateSeedPolicy::ByImporter { policies, .. } => (
            UpdateReuseScope::All,
            policies
                .iter()
                .map(|(importer_id, policy)| {
                    let scope = match policy {
                        ImporterUpdateSeedPolicy::DropAll => UpdateReuseScope::None,
                        ImporterUpdateSeedPolicy::DropOnly(targets) => {
                            UpdateReuseScope::Except(targets.clone())
                        }
                    };
                    (importer_id.clone(), scope)
                })
                .collect(),
        ),
    }
}
pub(super) fn full_resolution_required<'a>(
    has_reusable_seed: bool,
    importer_ids: impl IntoIterator<Item = &'a str>,
    default_scope: &pnpm_resolving_deps_resolver::UpdateReuseScope,
    scopes_by_importer: &BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
) -> bool {
    use pnpm_resolving_deps_resolver::UpdateReuseScope;

    !has_reusable_seed
        || importer_ids.into_iter().all(|importer_id| {
            let scope = if matches!(default_scope, UpdateReuseScope::None) {
                default_scope
            } else {
                scopes_by_importer.get(importer_id).unwrap_or(default_scope)
            };
            matches!(scope, UpdateReuseScope::None)
        })
}
