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
    pub(crate) optional_exclusions: OptionalDependencyExclusions<'a>,
    pub(crate) parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
}

/// Which of the project's `optionalDependencies` the comparison leaves out.
#[derive(Clone, Copy)]
pub(crate) struct OptionalDependencyExclusions<'a> {
    /// The configured `ignoredOptionalDependencies` patterns.
    pub(crate) ignored: &'a pnpm_matcher::Matcher,
    /// See [`super::FreshnessScope::allow_unresolved_optional_dependencies`].
    pub(crate) allow_unresolved: bool,
}

pub(crate) fn check_importer_satisfies(
    check: &ImporterSatisfactionCheck<'_>,
) -> Result<Vec<(String, String)>, FreshnessCheckError> {
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
    let (ignored_set, unresolved) =
        excluded_optional_dependency_names(check, manifest_for_freshness, importer);
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
    })?;

    check_directory_dependencies_freshness(check, importer)?;

    Ok(unresolved)
}
/// The optional dependencies the comparison leaves out: the configured
/// `ignoredOptionalDependencies`, plus the unresolved ones when the check
/// allows them.
fn excluded_optional_dependency_names(
    check: &ImporterSatisfactionCheck<'_>,
    manifest: &PackageManifest,
    importer: &pnpm_lockfile::ProjectSnapshot,
) -> (std::collections::HashSet<String>, Vec<(String, String)>) {
    let exclusions = check.optional_exclusions;
    let mut names = ignored_optional_dependency_names(manifest, exclusions.ignored);
    let mut unresolved = Vec::new();
    if exclusions.allow_unresolved {
        for (name, specifier) in
            unresolved_optional_dependencies_of(manifest, importer, exclusions.ignored)
        {
            names.insert(name.to_string());
            unresolved.push((name.to_string(), specifier.to_string()));
        }
    }
    (names, unresolved)
}

fn unresolved_optional_dependencies_of<'m>(
    manifest: &'m PackageManifest,
    importer: &'m pnpm_lockfile::ProjectSnapshot,
    ignored_optional_matcher: &'m pnpm_matcher::Matcher,
) -> impl Iterator<Item = (&'m str, &'m str)> + 'm {
    manifest
        .dependencies([DependencyGroup::Optional])
        .filter(move |(name, _)| {
            !ignored_optional_matcher.matches(name) && !crate::snapshot_has_alias(importer, name)
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

fn check_directory_dependencies_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    importer: &pnpm_lockfile::ProjectSnapshot,
) -> Result<(), FreshnessCheckError> {
    if check.lockfile.packages.is_none() || check.lockfile.snapshots.is_none() {
        return Ok(());
    }
    for group in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        if let Some(dep_map) = importer.get_map_by_group(group) {
            for (dep_name, dep_spec) in dep_map {
                check_single_dep_spec_directory_freshness(check, dep_name, dep_spec)?;
            }
        }
    }
    Ok(())
}

fn check_single_dep_spec_directory_freshness(
    check: &ImporterSatisfactionCheck<'_>,
    dep_name: &pnpm_lockfile::PkgName,
    dep_spec: &pnpm_lockfile::ResolvedDependencySpec,
) -> Result<(), FreshnessCheckError> {
    if matches!(dep_spec.version, pnpm_lockfile::ImporterDepVersion::Link(_)) {
        return Ok(());
    }
    let Some(pkg_key) = dep_spec.version.resolved_key(dep_name) else {
        return Ok(());
    };
    let Some(pkg_meta) = check.lockfile.packages
        .as_ref()
        .and_then(|p| p.get(&pkg_key))
    else {
        return Ok(());
    };
    if let pnpm_lockfile::LockfileResolution::Directory(dir_res) = &pkg_meta.resolution {
        let local_dep_dir = check.lockfile_dir.join(&dir_res.directory);
        let snapshot = check.lockfile.snapshots
            .as_ref()
            .and_then(|s| s.get(&pkg_key));
        check_single_directory_dep_freshness(
            &dep_name.to_string(),
            &dir_res.directory,
            &local_dep_dir,
            snapshot,
        )?;
    }
    Ok(())
}

fn check_single_directory_dep_freshness(
    dep_name: &str,
    rel_path: &str,
    local_dep_dir: &Path,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
) -> Result<(), FreshnessCheckError> {
    let local_manifest = pnpm_workspace::safe_read_project_manifest_only(local_dep_dir)
        .ok()
        .flatten()
        .ok_or_else(|| {
            FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            })
        })?;
    let Some(snapshot) = snapshot else {
        return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
            name: dep_name.to_string(),
            path: rel_path.to_string(),
        }));
    };
    check_local_dep_group_freshness(
        dep_name,
        rel_path,
        &local_manifest,
        DependencyGroup::Prod,
        snapshot.dependencies.as_ref(),
    )?;
    check_local_dep_group_freshness(
        dep_name,
        rel_path,
        &local_manifest,
        DependencyGroup::Optional,
        snapshot.optional_dependencies.as_ref(),
    )?;
    Ok(())
}

fn check_local_dep_group_freshness(
    dep_name: &str,
    rel_path: &str,
    local_manifest: &PackageManifest,
    group: DependencyGroup,
    snapshot_deps: Option<
        &std::collections::HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
    >,
) -> Result<(), FreshnessCheckError> {
    let manifest_deps: std::collections::HashMap<&str, &str> = local_manifest
        .dependencies([group])
        .collect();
    if let Some(snapshot_deps) = snapshot_deps {
        check_snapshot_keys_in_manifest(dep_name, rel_path, &manifest_deps, snapshot_deps)?;
    }
    check_manifest_specs_satisfy_snapshot(dep_name, rel_path, &manifest_deps, snapshot_deps)
}

fn check_snapshot_keys_in_manifest(
    dep_name: &str,
    rel_path: &str,
    manifest_deps: &std::collections::HashMap<&str, &str>,
    snapshot_deps: &std::collections::HashMap<
        pnpm_lockfile::PkgName,
        pnpm_lockfile::SnapshotDepRef,
    >,
) -> Result<(), FreshnessCheckError> {
    for lockfile_dep_name in snapshot_deps.keys() {
        let lockfile_name_str = lockfile_dep_name.to_string();
        if !manifest_deps.contains_key(lockfile_name_str.as_str()) {
            return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            }));
        }
    }
    Ok(())
}

fn check_manifest_specs_satisfy_snapshot(
    dep_name: &str,
    rel_path: &str,
    manifest_deps: &std::collections::HashMap<&str, &str>,
    snapshot_deps: Option<
        &std::collections::HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
    >,
) -> Result<(), FreshnessCheckError> {
    for (name, spec) in manifest_deps {
        let lockfile_dep = snapshot_deps.and_then(|deps| {
            pnpm_lockfile::PkgName::parse(*name)
                .ok()
                .and_then(|n| deps.get(&n))
        });
        let Some(lockfile_dep) = lockfile_dep else {
            return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            }));
        };
        if !spec_satisfies_snapshot_dep(spec, lockfile_dep) {
            return Err(FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
                name: dep_name.to_string(),
                path: rel_path.to_string(),
            }));
        }
    }
    Ok(())
}

fn spec_satisfies_snapshot_dep(spec: &str, lockfile_dep: &pnpm_lockfile::SnapshotDepRef) -> bool {
    if spec.starts_with("file:") || spec.starts_with("link:") || spec.starts_with("workspace:") {
        return true;
    }
    let clean_spec = if let Some(stripped) = spec.strip_prefix("npm:") {
        stripped
            .rfind('@')
            .map_or(stripped, |idx| &stripped[idx + 1..])
    } else {
        spec
    };
    let Ok(range) = clean_spec.parse::<node_semver::Range>() else {
        return true;
    };
    let Some(version) = lockfile_dep.ver_peer().and_then(|v| v.version_semver()) else {
        return true;
    };
    range.satisfies(version)
}
