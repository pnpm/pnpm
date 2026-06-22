use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_lockfile::{Lockfile, MaybeLazyLockfile};
use pacquet_modules_yaml::{Host, read_modules_layout, read_modules_manifest};
use pacquet_package_manager::{
    Install, RebuildOptions, UpdateSeedPolicy, allow_build_key_from_ignored_build,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use std::collections::HashSet;

use crate::State;

/// `pacquet rebuild` — re-run the lifecycle scripts of installed
/// dependencies. Ports pnpm's
/// [`rebuild`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/commands/src/build/rebuild.ts).
#[derive(Debug, Args)]
pub struct RebuildArgs {
    /// Rebuild only the named packages. With no names, every dependency
    /// that requires a build is rebuilt.
    pub packages: Vec<String>,

    /// Rebuild packages that were not built during installation. Packages
    /// are not built when installing with the `--ignore-scripts` flag.
    #[clap(long)]
    pub pending: bool,
}

impl RebuildArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let selected = resolve_selection(&self.packages, self.pending, state.config)?;
        run_rebuild::<Reporter>(&state, selected).await
    }
}

/// Resolve the rebuild's package selection. Explicit `packages` win;
/// otherwise `--pending` selects the packages `.modules.yaml` recorded as
/// not-yet-built; otherwise `None` rebuilds every build-needing package.
fn resolve_selection(
    packages: &[String],
    pending: bool,
    config: &Config,
) -> miette::Result<Option<Vec<String>>> {
    if !packages.is_empty() {
        return Ok(Some(packages.to_vec()));
    }
    if !pending {
        return Ok(None);
    }
    let modules = read_modules_manifest::<Host>(&config.modules_dir).into_diagnostic()?;
    let pending_names = modules
        .map(|manifest| {
            manifest
                .pending_builds
                .iter()
                .map(|dep_path| allow_build_key_from_ignored_build(dep_path))
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(pending_names))
}

/// Drive a forced rebuild of the selected packages (or every build-needing
/// package when `selected_names` is `None`) through the frozen-install
/// pipeline. Shared by `pacquet rebuild` and the rebuild step of
/// `pacquet approve-builds`.
pub(crate) async fn run_rebuild<Reporter: self::Reporter + 'static>(
    state: &State,
    selected_names: Option<Vec<String>>,
) -> miette::Result<()> {
    let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
        state;

    let lockfile_path = manifest.path().parent().map(|parent| parent.join(Lockfile::FILE_NAME));

    let rebuild = RebuildOptions {
        selected_names: selected_names.map(|names| names.into_iter().collect::<HashSet<_>>()),
    };

    let dependency_groups = rebuild_dependency_groups(config)?;

    Install {
        tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
        http_client,
        http_client_arc: std::sync::Arc::clone(http_client),
        config,
        manifest,
        lockfile: MaybeLazyLockfile::Lazy(lockfile),
        lockfile_path: lockfile_path.as_deref(),
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
        // `rebuild` re-runs dependency build scripts, not the root
        // project's own lifecycle scripts — matching pnpm's `buildProjects`.
        is_full_install: false,
        resolved_packages,
        supported_architectures: config.supported_architectures.clone(),
        node_linker: config.node_linker,
        lockfile_only: false,
        dry_run: false,
        update_seed_policy: UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
        catalogs_override: None,
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
/// their lifecycle scripts run. Falls back to every group only when there
/// is no `.modules.yaml` (nothing recorded to preserve).
fn rebuild_dependency_groups(config: &Config) -> miette::Result<Vec<DependencyGroup>> {
    let Some(modules) = read_modules_layout::<Host>(&config.modules_dir).into_diagnostic()? else {
        return Ok(vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]);
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
    Ok(groups)
}

#[cfg(test)]
mod tests;
