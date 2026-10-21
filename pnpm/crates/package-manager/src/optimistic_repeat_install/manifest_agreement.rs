//! Whether the manifests on disk still agree with the lockfile.

use super::{
    Config, DependencyGroup, ImporterDepVersion, Lockfile, OptimisticRepeatInstallCheck,
    PackageManifest, Path, PathBuf, ProjectSnapshot, WorkspaceState,
    current_lockfile::{
        CurrentLockfile, OUTDATED_DEPS, current_records_importer, selection_has_dependencies,
        selection_matches_wanted_lockfile,
    },
    freshness::{ProjectStatus, Stale},
    validation::manifest_digest,
};
use rayon::prelude::*;
use settings_drift::check_settings_match_lockfile;

mod settings_drift;

/// One project manifest and whether the last validation still proves the
/// project installed.
pub(crate) struct ManifestStat<'a> {
    pub(crate) root_dir: &'a Path,
    pub(crate) manifest: &'a PackageManifest,
    pub(crate) status: ProjectStatus,
}

/// Every project's [`ManifestStat`], with `status` judging each project from
/// its root directory and its manifest's digest.
pub(crate) fn manifest_stats<'a>(
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    status: impl Fn(&Path, String) -> ProjectStatus,
) -> Vec<ManifestStat<'a>> {
    project_manifests
        .iter()
        .map(|(root_dir, manifest)| ManifestStat {
            root_dir,
            manifest,
            status: status(root_dir, manifest_digest(manifest)),
        })
        .collect()
}

/// Check the `held` projects against the wanted lockfile: each manifest must
/// still be satisfied by it, and the current lockfile must record what it
/// materializes for each project whose install was never proven against it.
/// `current` stands in for a missing wanted lockfile.
pub(crate) fn held_projects_match_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    held: &[&ManifestStat<'_>],
    current: &CurrentLockfile<'_>,
    dedupe_peers: bool,
) -> Result<(), Stale> {
    let lockfile = check.lockfile
        .get()
        .map_err(|_| Stale::Content("the wanted lockfile cannot be read or parsed"))?;
    let wanted = match lockfile {
        Some(wanted) => {
            require_installed(check, state, wanted, held, current)?;
            wanted
        }
        // A `pnpm-lock.yaml` without a main document, such as one holding only
        // the config dependencies, is one the install has to complete. A
        // caller that hands over no lockfile says there is none.
        None if !matches!(check.lockfile, pnpm_lockfile::MaybeLazyLockfile::Loaded(_))
            && check.workspace_root.join(check.config.wanted_lockfile_name()).is_file() =>
        {
            return Err(Stale::Content("the wanted lockfile has no dependency graph"));
        }
        // The current lockfile stands in for the wanted one, so there is no
        // second lockfile for an install to fall behind.
        None => {
            let current = current
                .get()
                .ok_or(Stale::Content("the current lockfile cannot be loaded"))?;
            let Some(current) = current else {
                // Without either lockfile only a project with nothing to
                // install is current.
                return if held.iter().any(|stat| super::manifest_has_runtime_deps(stat.manifest)) {
                    Err(Stale::NoLockfile)
                } else {
                    Ok(())
                };
            };
            require_recorded_in_current(&unproven_projects(check, held), current)?;
            current
        }
    };
    check_projects_content(check, wanted, held, dedupe_peers).map_err(Stale::Content)
}

/// The current lockfile records what `wanted` materializes for each `held`
/// project whose install was never proven against `wanted`.
fn require_installed(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    wanted: &Lockfile,
    held: &[&ManifestStat<'_>],
    current: &CurrentLockfile<'_>,
) -> Result<(), Stale> {
    let unproven = unproven_projects(check, held);
    if unproven.is_empty() {
        return Ok(());
    }
    let current = current
        .get()
        .ok_or(Stale::Content("the current lockfile cannot be loaded"))?;
    if let Some(current) = current {
        require_recorded_in_current(&unproven, current)?;
    }
    let importer_ids = unproven
        .into_iter()
        .map(|(importer_id, _)| importer_id)
        .collect();
    match current {
        Some(current)
            if selection_matches_wanted_lockfile(check, state, wanted, current, &importer_ids) =>
        {
            Ok(())
        }
        Some(_) => Err(Stale::Content(OUTDATED_DEPS)),
        None if selection_has_dependencies(wanted, &importer_ids) => Err(Stale::NothingInstalled),
        None => Ok(()),
    }
}

fn unproven_projects<'a>(
    check: &OptimisticRepeatInstallCheck<'_>,
    held: &[&'a ManifestStat<'a>],
) -> Vec<(String, &'a ManifestStat<'a>)> {
    held.iter()
        .filter(|stat| stat.status.needs_install_proof())
        .map(|stat| {
            (pnpm_workspace::importer_id_from_root_dir(check.workspace_root, stat.root_dir), *stat)
        })
        .collect()
}

fn require_recorded_in_current(
    unproven: &[(String, &ManifestStat<'_>)],
    current: &Lockfile,
) -> Result<(), Stale> {
    if let Some((_, stat)) = unproven
        .iter()
        .find(|(importer_id, stat)| {
            super::manifest_has_runtime_deps(stat.manifest)
                && !current_records_importer(current, importer_id, stat.status)
        })
    {
        return Err(Stale::ProjectNotInstalled(super::project_id(stat.root_dir, stat.manifest)));
    }
    Ok(())
}

/// The full content check of the modified projects against the wanted
/// lockfile, once its settings are known not to have drifted.
pub(super) fn check_projects_content(
    check: &OptimisticRepeatInstallCheck<'_>,
    wanted: &Lockfile,
    to_check: &[&ManifestStat<'_>],
    dedupe_peers: bool,
) -> Result<(), &'static str> {
    let parsed_overrides = crate::install::parse_config_overrides(check.config, check.catalogs)
        .map_err(|_| "pnpm.overrides cannot be parsed")?;
    check_settings_match_lockfile(check, wanted, parsed_overrides.as_deref(), dedupe_peers)?;

    let linked_ctx = LinkedPackagesContext::new(check.config, check.project_manifests);
    let dependency_manifests = check.manifests.dependency_manifests;
    let workspace_packages = crate::install::workspace_packages_for_freshness(
        check.config,
        check.is_workspace_install,
        check.project_manifests,
        dependency_manifests,
    );
    let ignored_optional_matcher = pnpm_matcher::create_matcher(
        check.config.ignored_optional_dependencies.as_deref().unwrap_or_default(),
    );
    let project_manifests = crate::install::project_manifests_by_dir(
        check.project_manifests.iter().map(|(_, manifest)| *manifest),
        dependency_manifests,
    );
    let content_check = ProjectContentCheck {
        workspace_root: check.workspace_root,
        config: check.config,
        wanted,
        linked_ctx: &linked_ctx,
        workspace: crate::install::WorkspaceProjects {
            packages: workspace_packages.as_ref(),
            manifests_by_dir: &project_manifests,
            catalogs: check.catalogs,
            canonical_paths: &pnpm_fs::CanonicalPaths::default(),
        },
        ignored_optional_matcher: &ignored_optional_matcher,
        parsed_overrides: parsed_overrides.as_deref(),
        overrider: parsed_overrides
            .as_deref()
            .map(|parsed| crate::VersionsOverrider::new(parsed, check.workspace_root)),
    };
    // Collect before returning so errors remain ordered by project.
    let results: Vec<Result<(), &'static str>> = to_check
        .par_iter()
        .map(|project| project_content_check(&content_check, project))
        .collect();
    results.into_iter().collect()
}

/// What one modified project is checked against.
struct ProjectContentCheck<'a> {
    workspace_root: &'a Path,
    config: &'a Config,
    wanted: &'a Lockfile,
    linked_ctx: &'a LinkedPackagesContext<'a>,
    workspace: crate::install::WorkspaceProjects<'a>,
    ignored_optional_matcher: &'a pnpm_matcher::Matcher,
    parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    overrider: Option<crate::VersionsOverrider>,
}

fn project_content_check(
    context: &ProjectContentCheck<'_>,
    project: &ManifestStat<'_>,
) -> Result<(), &'static str> {
    let importer_id =
        pnpm_workspace::importer_id_from_root_dir(context.workspace_root, project.root_dir);
    let importer = context.wanted.importers.get(&importer_id);
    if let Err(error) =
        crate::install::check_importer_satisfies(&crate::install::ImporterSatisfactionCheck {
            lockfile: context.wanted,
            lockfile_dir: context.workspace_root,
            manifest: project.manifest,
            importer_id: &importer_id,
            config: context.config,
            workspace: context.workspace,
            optional_exclusions: crate::install::OptionalDependencyExclusions {
                ignored: context.ignored_optional_matcher,
                frozen_lockfile: false,
            },
            parsed_overrides: context.parsed_overrides,
        })
    {
        if is_outdated_linked_package(context, project, importer, &error) {
            return Err("a linked package is out of date");
        }
        tracing::debug!(target: "pacquet::install", %error, importer_id, "repeat-install content check: manifest no longer satisfied");
        return Err("a modified manifest is no longer satisfied by the lockfile");
    }
    let Some(importer) = importer else {
        return Err("a modified project has no importer entry in the lockfile");
    };
    let manifest = overridden_manifest(context, project);
    if !linked_packages_are_up_to_date(context.linked_ctx, project.root_dir, &manifest, importer) {
        return Err("a linked package is out of date");
    }
    Ok(())
}

fn is_outdated_linked_package(
    context: &ProjectContentCheck<'_>,
    project: &ManifestStat<'_>,
    importer: Option<&ProjectSnapshot>,
    error: &crate::install::FreshnessCheckError,
) -> bool {
    matches!(
        error,
        crate::install::FreshnessCheckError::Stale(
            pnpm_lockfile::StalenessReason::LocalDependencyOutdated { .. },
        ),
    ) && importer.is_some_and(|imp| {
        let manifest = overridden_manifest(context, project);
        !linked_packages_are_up_to_date(context.linked_ctx, project.root_dir, &manifest, imp)
    })
}

fn overridden_manifest<'a>(
    context: &ProjectContentCheck<'_>,
    project: &ManifestStat<'a>,
) -> std::borrow::Cow<'a, PackageManifest> {
    let Some(overrider) = &context.overrider else {
        return std::borrow::Cow::Borrowed(project.manifest);
    };
    let mut cloned = project.manifest.clone();
    overrider.apply(&mut cloned, Some(project.root_dir));
    std::borrow::Cow::Owned(cloned)
}

/// Shared lookups for [`linked_packages_are_up_to_date`], built once
/// per content check.
pub(crate) struct LinkedPackagesContext<'a> {
    pub(crate) link_workspace_packages: bool,
    pub(crate) manifests_by_dir: std::collections::HashMap<&'a Path, &'a PackageManifest>,
    /// `name → version → root_dir` over the workspace's projects.
    pub(crate) workspace_packages:
        std::collections::HashMap<String, std::collections::HashMap<String, &'a Path>>,
}

/// Verify that linked packages are up to date: every importer
/// dependency that resolved to a workspace link must still link under
/// today's manifest spec, and every one that resolved to the registry
/// must not have become linkable. The local-file-dependency freshness
/// branch (a `file:` directory specifier) is not handled here — those
/// entries conservatively report "not up to date" so the full install
/// path re-evaluates them.
pub(crate) fn linked_packages_are_up_to_date(
    ctx: &LinkedPackagesContext<'_>,
    project_dir: &Path,
    manifest: &PackageManifest,
    snapshot: &ProjectSnapshot,
) -> bool {
    const GROUPS: [(DependencyGroup, &str); 3] = [
        (DependencyGroup::Optional, "optionalDependencies"),
        (DependencyGroup::Prod, "dependencies"),
        (DependencyGroup::Dev, "devDependencies"),
    ];
    for (group, manifest_field) in GROUPS {
        let Some(lockfile_deps) = snapshot.get_map_by_group(group) else {
            continue;
        };
        let Some(manifest_deps) = manifest
            .value()
            .get(manifest_field)
            .and_then(|value| value.as_object())
        else {
            continue;
        };
        if !linked_group_is_up_to_date(ctx, project_dir, lockfile_deps, manifest_deps) {
            return false;
        }
    }
    true
}

fn linked_group_is_up_to_date(
    ctx: &LinkedPackagesContext<'_>,
    project_dir: &Path,
    lockfile_deps: &pnpm_lockfile::ResolvedDependencyMap,
    manifest_deps: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    for (dep_name, dep) in lockfile_deps {
        let dep_name = dep_name.to_string();
        let Some(current_spec) = manifest_deps.get(&dep_name).and_then(|v| v.as_str()) else {
            continue;
        };
        if !linked_dep_is_up_to_date(ctx, project_dir, &dep_name, dep, current_spec) {
            return false;
        }
    }
    true
}

fn linked_dep_is_up_to_date(
    ctx: &LinkedPackagesContext<'_>,
    project_dir: &Path,
    dep_name: &str,
    dep: &pnpm_lockfile::ResolvedDependencySpec,
    current_spec: &str,
) -> bool {
    if crate::install::is_declared_local_directory(&dep.specifier) {
        // A `file:` specifier that resolved to `link:` (e.g. an
        // injected self-reference) is a local link with no
        // `packages:` entry — up to date by construction.
        return matches!(dep.version, ImporterDepVersion::Link(_));
    }
    if matches!(dep.version, ImporterDepVersion::File(_)) {
        // An injected workspace project: a `file:` copy, not a link. Its own
        // dependencies were compared with its snapshot by the directory
        // freshness check `check_importer_satisfies` ran before this one.
        return true;
    }
    let link_target = dep.version.as_link_target();
    let is_linked = link_target.is_some();
    if is_linked && linked_spec_needs_no_resolution(current_spec) {
        return true;
    }
    let (target_name, available_range) = crate::install::spec_target_and_range(current_spec);
    let package_name = target_name.unwrap_or(dep_name);
    let Some(linked_dir) = linked_package_dir(ctx, project_dir, package_name, dep, link_target)
    else {
        return true;
    };
    if !ctx.link_workspace_packages && !current_spec.starts_with("workspace:") {
        // A linkable dir exists, but nothing requests linking it.
        return true;
    }
    let local_package_satisfies_range = matches!(available_range, "*" | "^" | "~")
        || ctx
            .linked_version(&linked_dir)
            .is_some_and(|version| semver_satisfies_loosely(&version, available_range));
    is_linked == local_package_satisfies_range
}

/// A declaration that asks for a link outright, or names a distribution tag,
/// leaves a linked dependency up to date without resolving anything.
fn linked_spec_needs_no_resolution(current_spec: &str) -> bool {
    current_spec.starts_with("link:")
        || current_spec.starts_with("file:")
        || current_spec.starts_with("workspace:.")
        || spec_is_distribution_tag(current_spec)
}

/// The directory a dependency would be linked from: its own link target, or
/// the workspace project whose version the lockfile recorded.
fn linked_package_dir<'a>(
    ctx: &'a LinkedPackagesContext<'_>,
    project_dir: &Path,
    dep_name: &str,
    dep: &pnpm_lockfile::ResolvedDependencySpec,
    link_target: Option<&str>,
) -> Option<std::borrow::Cow<'a, Path>> {
    match link_target {
        Some(target) => Some(std::borrow::Cow::Owned(project_dir.join(target))),
        None => dep.version
            .as_regular()
            .map(std::string::ToString::to_string)
            .and_then(|version| ctx.workspace_packages.get(dep_name)?.get(&version))
            .map(|dir| std::borrow::Cow::Borrowed(*dir)),
    }
}

/// Whether a bare specifier is an npm distribution tag (`latest`,
/// `beta`, ...): anything that doesn't parse as a semver range and
/// contains only characters a tag name may carry. Protocol-ish specs
/// (`workspace:^1.0.0`, `npm:foo@1`) contain `:`/`@`/`/` and therefore
/// never match.
pub(crate) fn spec_is_distribution_tag(spec: &str) -> bool {
    !spec.is_empty()
        && spec.parse::<node_semver::Range>().is_err()
        && spec
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
}

/// `semver.satisfies(version, range, { loose: true })` — a version or
/// range that doesn't parse fails the match.
pub(crate) fn semver_satisfies_loosely(version: &str, range: &str) -> bool {
    let Ok(version) = version.parse::<node_semver::Version>() else { return false };
    let Ok(range) = range.parse::<node_semver::Range>() else { return false };
    range.satisfies(&version)
}
