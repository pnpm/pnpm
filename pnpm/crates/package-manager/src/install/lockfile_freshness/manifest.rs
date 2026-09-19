use super::{
    super::{
        Config, DependencyGroup, Lockfile, PackageManifest, Path, StalenessReason,
        satisfies_package_manifest,
    },
    FreshnessCheckError,
};

/// Per-importer slice of the freshness gate: the manifest of the
/// project at `importer_id` must still be satisfied by the lockfile's
/// importer snapshot.
///
/// `lockfile_dir` is the directory the lockfile was written from. A
/// relative `link:` / `file:` override target names a path from there,
/// so anchoring the overrider anywhere else rewrites the manifest to
/// specifiers the lockfile never recorded.
pub(crate) struct ImporterSatisfactionCheck<'a> {
    pub(crate) lockfile: &'a Lockfile,
    pub(crate) lockfile_dir: &'a Path,
    pub(crate) manifest: &'a PackageManifest,
    pub(crate) importer_id: &'a str,
    pub(crate) config: &'a Config,
    pub(crate) workspace_packages: Option<&'a pnpm_resolving_resolver_base::WorkspacePackages>,
    pub(crate) ignored_optional_matcher: &'a pnpm_matcher::Matcher,
    pub(crate) parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
}

pub(crate) fn check_importer_satisfies(
    check: &ImporterSatisfactionCheck<'_>,
) -> Result<(), FreshnessCheckError> {
    let importer = check.lockfile.importers
        .get(check.importer_id)
        .ok_or_else(|| FreshnessCheckError::NoImporter {
            importer_id: check.importer_id.to_string(),
        })?;

    // Apply `pnpm.overrides` to a *cloned* manifest before the
    // per-importer specifier check so the lockfile's specifiers —
    // written with overrides already applied — match the on-disk
    // manifest's deps. The caller's manifest stays pristine since the
    // override pass conceptually returns a new manifest
    // from the perspective of every consumer downstream of the
    // resolver.
    // `auto_install_peers` is folded into `satisfies_package_manifest`
    // itself, so the manifest is cloned here only for the two mutations the
    // comparison needs done up front: applying `pnpm.overrides` and dropping
    // `link:` deps under `exclude_links_from_lockfile`.
    let normalized_manifest = normalized_freshness_manifest(
        check.manifest,
        check.config,
        check.workspace_packages,
        importer,
        check.parsed_overrides,
        check.lockfile_dir,
    );
    let manifest_for_freshness = normalized_manifest.as_ref();

    // Build the `ignoredOptionalDependencies` filter set: iterate
    // `manifest.optionalDependencies` and delete matches from BOTH the
    // `optional` and `dependencies` maps. A name only present in
    // `dependencies` that happens to match the
    // pattern is NOT removed — set-based ("name was in
    // optionalDependencies AND matched") rather than pure pattern
    // matching. `devDependencies` is untouched on purpose; the group
    // gate inside `satisfies_package_manifest` enforces that.
    let ignored_set =
        ignored_optional_dependency_names(manifest_for_freshness, check.ignored_optional_matcher);
    let is_ignored_optional: &dyn Fn(&str) -> bool = &|name: &str| ignored_set.contains(name);

    satisfies_package_manifest(
        importer,
        manifest_for_freshness,
        check.config.auto_install_peers,
        is_ignored_optional,
    )
    .map_err(|reason| {
        // Stamp the importer onto a specifier diff so the workspace-wide
        // freshness report names the drifted project, not only the dep.
        let reason = match reason {
            StalenessReason::SpecifiersDiffer(mut diff) => {
                diff.importer_id = Some(check.importer_id.to_string());
                StalenessReason::SpecifiersDiffer(diff)
            }
            other => other,
        };
        FreshnessCheckError::Stale(reason)
    })
}
pub(in super::super) fn ignored_optional_dependency_names(
    manifest: &PackageManifest,
    matcher: &pnpm_matcher::Matcher,
) -> std::collections::HashSet<String> {
    manifest
        .dependencies([pnpm_package_manifest::DependencyGroup::Optional])
        .filter(|(name, _)| matcher.matches(name))
        .map(|(name, _)| name.to_string())
        .collect()
}
pub(in super::super) fn manifest_has_effective_dependencies(
    manifest: &PackageManifest,
    ignored_optional_matcher: &pnpm_matcher::Matcher,
) -> bool {
    if manifest
        .dependencies([pnpm_package_manifest::DependencyGroup::Dev])
        .next()
        .is_some()
    {
        return true;
    }
    let ignored = ignored_optional_dependency_names(manifest, ignored_optional_matcher);
    manifest
        .dependencies([
            pnpm_package_manifest::DependencyGroup::Prod,
            pnpm_package_manifest::DependencyGroup::Optional,
        ])
        .any(|(name, _)| !ignored.contains(name))
}
pub(in super::super) fn exclude_linked_dependencies(
    manifest: &mut PackageManifest,
    workspace_packages: Option<&pnpm_resolving_resolver_base::WorkspacePackages>,
    importer: Option<&pnpm_lockfile::ProjectSnapshot>,
) {
    let Some(manifest) = manifest.value_mut().as_object_mut() else {
        return;
    };
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        let group: &str = group.into();
        let Some(dependencies) = manifest.get_mut(group).and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        dependencies.retain(|alias, specifier| {
            retain_in_freshness_manifest(workspace_packages, importer, alias, specifier)
        });
    }
}
fn retain_in_freshness_manifest(
    workspace_packages: Option<&pnpm_resolving_resolver_base::WorkspacePackages>,
    importer: Option<&pnpm_lockfile::ProjectSnapshot>,
    alias: &str,
    specifier: &serde_json::Value,
) -> bool {
    let Some(specifier) = specifier.as_str() else {
        return true;
    };
    if specifier.starts_with("link:") {
        return false;
    }
    let Some(workspace_packages) = workspace_packages else {
        return true;
    };
    specifier.starts_with("workspace:")
        || importer.is_some_and(|snapshot| crate::snapshot_has_alias(snapshot, alias))
        || crate::workspace_link_target(workspace_packages, alias, specifier).is_none()
}
// Only overrides and excluded links require a clone; all other freshness checks borrow the manifest.
pub(super) fn normalized_freshness_manifest<'a>(
    manifest: &'a PackageManifest,
    config: &Config,
    workspace_packages: Option<&pnpm_resolving_resolver_base::WorkspacePackages>,
    importer: &pnpm_lockfile::ProjectSnapshot,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
    lockfile_dir: &Path,
) -> std::borrow::Cow<'a, PackageManifest> {
    if parsed_overrides.is_none() && !config.exclude_links_from_lockfile {
        return std::borrow::Cow::Borrowed(manifest);
    }
    let project_dir = manifest
        .path()
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut cloned = manifest.clone();
    if let Some(parsed) = parsed_overrides {
        crate::VersionsOverrider::new(parsed, lockfile_dir).apply(&mut cloned, Some(project_dir));
    }
    if config.exclude_links_from_lockfile {
        exclude_linked_dependencies(&mut cloned, workspace_packages, Some(importer));
    }
    std::borrow::Cow::Owned(cloned)
}
