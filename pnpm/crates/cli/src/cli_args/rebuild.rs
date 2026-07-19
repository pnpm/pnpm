use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_lockfile::{MaybeLazyLockfile, PackageKey};
use pacquet_modules_yaml::{Host, read_modules_layout, read_modules_manifest};
use pacquet_package_manager::{
    Install, RebuildOptions, UpdateSeedPolicy, allow_build_key_from_ignored_build,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use std::collections::HashSet;

use crate::State;

/// `pacquet rebuild` — re-run the lifecycle scripts of installed
/// dependencies.
#[derive(Debug, Args)]
pub struct RebuildArgs {
    /// Rebuild only the named packages. With no names, every dependency
    /// that requires a build is rebuilt.
    pub packages: Vec<String>,

    /// Rebuild packages that were not built during installation, such as
    /// under `--ignore-scripts`.
    #[clap(long)]
    pub pending: bool,
}

impl RebuildArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let selection = resolve_selection(&self.packages, self.pending, state.config)?;
        run_rebuild::<Reporter>(&state, selection).await
    }
}

/// What a `pacquet rebuild` invocation was asked to rebuild.
#[derive(Debug, Default)]
pub(crate) struct RebuildSelection {
    /// Allow-build keys of the dependencies to rebuild, or `None` for
    /// every build-needing package.
    pub(crate) names: Option<Vec<String>>,
    /// Importer ids whose own deferred install scripts to run. Only
    /// `--pending` populates this — see
    /// [`RebuildOptions::pending_projects`].
    pub(crate) projects: Vec<String>,
}

/// Resolve the rebuild's selection. Explicit `packages` win; otherwise
/// `--pending` selects what `.modules.yaml` recorded as not-yet-built;
/// otherwise every build-needing package.
///
/// A `pendingBuilds` record is a dep path for a dependency and an
/// importer id for a workspace project, so the two are told apart by
/// whether the entry parses as a package key.
fn resolve_selection(
    packages: &[String],
    pending: bool,
    config: &Config,
) -> miette::Result<RebuildSelection> {
    if !packages.is_empty() {
        return Ok(RebuildSelection { names: Some(packages.to_vec()), projects: Vec::new() });
    }
    if !pending {
        return Ok(RebuildSelection::default());
    }
    let Some(modules) = read_modules_manifest::<Host>(&config.modules_dir).into_diagnostic()?
    else {
        return Ok(RebuildSelection { names: Some(Vec::new()), projects: Vec::new() });
    };
    let (dep_paths, projects): (Vec<&String>, Vec<&String>) =
        modules.pending_builds.iter().partition(|entry| entry.parse::<PackageKey>().is_ok());
    Ok(RebuildSelection {
        names: Some(
            dep_paths
                .into_iter()
                .map(|dep_path| allow_build_key_from_ignored_build(dep_path))
                .collect(),
        ),
        projects: projects.into_iter().cloned().collect(),
    })
}

/// Drive a forced rebuild of the selected packages (or every build-needing
/// package when `selected_names` is `None`) through the frozen-install
/// pipeline. Shared by `pacquet rebuild` and the rebuild step of
/// `pacquet approve-builds`.
pub(crate) async fn run_rebuild<Reporter: self::Reporter + 'static>(
    state: &State,
    selection: RebuildSelection,
) -> miette::Result<()> {
    let lockfile_path = state.lockfile_path();
    let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
        state;

    let rebuild = RebuildOptions {
        selected_names: selection.names.map(|names| names.into_iter().collect::<HashSet<_>>()),
        pending_projects: selection.projects,
    };

    let dependency_groups = rebuild_dependency_groups(config)?;

    Install {
        tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
        http_client,
        http_client_arc: std::sync::Arc::clone(http_client),
        config,
        manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Lazy(lockfile),
        lockfile_path: Some(&lockfile_path),
        // Reuse exactly the dependency groups the current `node_modules`
        // was materialized with, so a rebuild never widens the installed
        // set (see [`rebuild_dependency_groups`]).
        dependency_groups,
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: config.skip_runtimes,
        trust_lockfile: config.trust_lockfile,
        update_checksums: false,
        // `rebuild` re-runs dependency build scripts. The root
        // project's own lifecycle scripts run only for the importers
        // `--pending` names (see `RebuildOptions::pending_projects`).
        is_full_install: false,
        installs_only: true,
        resolved_packages,
        supported_architectures: config.supported_architectures.clone(),
        node_linker: config.node_linker,
        lockfile_only: false,
        dry_run: false,
        update_seed_policy: UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run_rebuild::<Reporter>(rebuild)
    .await
    .wrap_err("rebuilding dependencies")?;

    Ok(())
}

/// The dependency groups the current `node_modules` was materialized with,
/// read from `.modules.yaml`'s `included`. A rebuild must keep the
/// installed set unchanged, so it reuses exactly these groups rather than
/// widening to all of them — otherwise a `--prod` / `--no-optional`
/// install would have its excluded dev/optional dependencies fetched and
/// their lifecycle scripts run. Falls back to every group when there is no
/// `.modules.yaml`, or when it records no included groups (a legacy
/// manifest written before `included` existed, or a corrupt one) — neither
/// is a recorded "include nothing" intent to preserve.
fn rebuild_dependency_groups(config: &Config) -> miette::Result<Vec<DependencyGroup>> {
    const ALL: [DependencyGroup; 3] =
        [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let Some(modules) = read_modules_layout::<Host>(&config.modules_dir).into_diagnostic()? else {
        return Ok(ALL.to_vec());
    };
    let included = modules.included;
    let mut groups = Vec::with_capacity(3);
    if included.dependencies {
        groups.push(DependencyGroup::Prod);
    }
    if included.dev_dependencies {
        groups.push(DependencyGroup::Dev);
    }
    if included.optional_dependencies {
        groups.push(DependencyGroup::Optional);
    }
    // An all-false `included` would otherwise narrow the rebuild to no
    // groups and persist that empty state into `.modules.yaml` and the
    // current lockfile, breaking later installs.
    Ok(if groups.is_empty() { ALL.to_vec() } else { groups })
}

#[cfg(test)]
mod tests;
