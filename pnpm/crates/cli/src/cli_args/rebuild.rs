use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_lockfile::MaybeLazyLockfile;
use pacquet_modules_yaml::{Host, read_modules_layout, read_modules_manifest};
use pacquet_package_manager::{
    Install, RebuildOptions, UpdateSeedPolicy, allow_build_key_from_ignored_build,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::{
    State,
    cli_args::pipelines::{
        InstallFamilySelection, anchor_dedicated_project_config, select_workspace_projects,
    },
};

/// `pacquet rebuild` — re-run the lifecycle scripts of installed
/// dependencies.
#[derive(Debug, Clone, Args)]
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
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        workspace_selection: Option<InstallFamilySelection>,
    ) -> miette::Result<()> {
        let selection = resolve_selection(&self.packages, self.pending, state.config)?;
        run_rebuild::<Reporter>(&state, selection, workspace_selection).await
    }

    pub async fn run_from_cli<Reporter: self::Reporter + 'static>(
        self,
        cfg: &'static Config,
        prefix: PathBuf,
        manifest_path: PathBuf,
        recursive_sort: bool,
        no_bail: bool,
    ) -> miette::Result<()> {
        let workspace_selection =
            select_workspace_projects(cfg, &prefix, &manifest_path, recursive_sort, false)?;
        if workspace_selection.as_ref().is_some_and(|selection| selection.selected_dirs.is_empty())
        {
            return Ok(());
        }
        if !cfg.shared_workspace_lockfile
            && let Some(workspace_selection) = workspace_selection
        {
            let base_config = cfg.clone();
            let concurrency =
                usize::try_from(cfg.workspace_concurrency).unwrap_or(usize::MAX).max(1);
            let mut first_error = None;
            for group in workspace_selection.ordered_groups {
                for batch in group.chunks(concurrency) {
                    let rebuilds = batch.iter().cloned().map(|project_dir| {
                        let args = self.clone();
                        let mut project_config = base_config.clone();
                        anchor_dedicated_project_config(&mut project_config, &project_dir);
                        async move {
                            let project_config = Config::leak(project_config);
                            let state =
                                State::init(project_dir.join("package.json"), project_config, true)
                                    .wrap_err_with(|| {
                                        format!(
                                            "initialize the rebuild state for {}",
                                            project_dir.display(),
                                        )
                                    })?;
                            Box::pin(args.run::<Reporter>(state, None)).await
                        }
                    });
                    for result in futures_util::future::join_all(rebuilds).await {
                        if let Err(error) = result {
                            if !no_bail {
                                return Err(error);
                            }
                            first_error.get_or_insert(error);
                        }
                    }
                }
            }
            if let Some(error) = first_error {
                return Err(error);
            }
            return Ok(());
        }

        let state =
            State::init(manifest_path, cfg, true).wrap_err("initialize the rebuild state")?;
        Box::pin(self.run::<Reporter>(state, workspace_selection)).await
    }
}

/// What a `pacquet rebuild` invocation was asked to rebuild.
#[derive(Debug, Default)]
pub(crate) struct RebuildSelection {
    /// Allow-build keys of the dependencies to rebuild, or `None` for
    /// every build-needing package.
    pub(crate) names: Option<Vec<String>>,
    /// [`RebuildOptions::pending_projects`]; only `--pending` populates it.
    pub(crate) projects: Vec<String>,
}

/// Resolve the rebuild's selection. Explicit `packages` win; otherwise
/// `--pending` selects what `.modules.yaml` recorded as not-yet-built;
/// otherwise every build-needing package.
///
/// A `pendingBuilds` record is a dep path for a dependency and an
/// importer id for a workspace project, and both are plain strings on
/// disk. They are told apart by whether the entry resolves to a
/// directory with a manifest, which is what an importer id means —
/// asking the string's shape instead would misread a workspace
/// directory named `foo@1.0.0` as a dependency and then drop its debt
/// without running anything.
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
    // `.modules.yaml` sits in the root `node_modules`, so its importer
    // ids are relative to that directory's parent.
    let lockfile_dir = config.modules_dir.parent().unwrap_or(&config.modules_dir);
    // An importer id is always a relative path; a dep path can be
    // absolute-looking (`/lodash@1.0.0`), and Rust's `Path::join`
    // replaces the base on an absolute component, so probe only for
    // relative entries — an absolute one is a dependency by construction.
    let is_project = |entry: &String| {
        !Path::new(entry).is_absolute() && lockfile_dir.join(entry).join("package.json").is_file()
    };
    let (projects, dep_paths): (Vec<&String>, Vec<&String>) =
        modules.pending_builds.iter().partition(|entry| is_project(entry));
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
    workspace_selection: Option<InstallFamilySelection>,
) -> miette::Result<()> {
    let lockfile_path = state.lockfile_path();
    let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
        state;

    let rebuild = RebuildOptions {
        selected_names: selection.names.map(|names| names.into_iter().collect::<HashSet<_>>()),
        pending_projects: selection.projects,
    };

    let dependency_groups = rebuild_dependency_groups(config)?;

    let install = Install {
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
    };
    match workspace_selection.as_ref() {
        Some(selection) => {
            install
                .run_selected_rebuild::<Reporter>(
                    pacquet_package_manager::WorkspaceInstallSelection {
                        all_projects: &selection.projects,
                        ordered_groups: &selection.ordered_groups,
                        ordered_dirs: &selection.ordered_dirs,
                        selected_dirs: selection.selected_dirs.as_ref(),
                        active_manifest_is_standin: selection.active_manifest_is_standin,
                    },
                    rebuild,
                )
                .await
        }
        None => install.run_rebuild::<Reporter>(rebuild).await,
    }
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
