//! What the fresh install is allowed to put on disk: the host it is
//! installing for, the engine name that keys its store slots, and the
//! set of snapshots it must skip.
//!
//! Reads the freshly-built lockfile and runs before
//! `CreateVirtualStore`, which materializes only what the skip set
//! leaves behind and addresses its slots by the engine name decided
//! here.

use super::InstallWithFreshLockfileError;
use crate::SkippedSnapshots;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_is_installable::SupportedArchitectures;
use pacquet_package_manifest::DependencyGroup;
use std::path::Path;

/// Detect the host the install's `os` / `cpu` / `libc` / `engines`
/// constraints are checked against.
///
/// `None` when nothing in the lockfile constrains installability, or
/// when `--force` bypasses the checks outright (see [`Config::force`]):
/// no skip set is computed and the hoisted walker emits every dep, so no
/// host detection is needed either.
pub(super) async fn detect_installability_host(
    config: &Config,
    lockfile: &Lockfile,
    node_version: Option<String>,
    supported_architectures: Option<&SupportedArchitectures>,
) -> Option<crate::InstallabilityHost> {
    let constrained = lockfile.packages.as_ref().is_some_and(|packages| {
        lockfile
            .snapshots
            .as_ref()
            .is_some_and(|snapshots| crate::any_installability_constraint(snapshots, packages))
    });
    if config.force || !constrained {
        return None;
    }

    let engine_strict = config.engine_strict;
    let mut host = match node_version {
        // An explicit `nodeVersion` needs no `node --version` probe, so
        // build the host directly off the reactor thread.
        node_version @ Some(_) => {
            crate::InstallabilityHost::detect_with(engine_strict, node_version)
        }
        None => tokio::task::spawn_blocking(move || {
            crate::InstallabilityHost::detect_with(engine_strict, None)
        })
        .await
        .ok()
        .unwrap_or_else(|| crate::InstallabilityHost {
            node_version: "99999.0.0".to_string(),
            node_detected: false,
            os: pacquet_graph_hasher::host_platform(),
            cpu: pacquet_graph_hasher::host_arch(),
            libc: pacquet_graph_hasher::host_libc(),
            supported_architectures: None,
            engine_strict,
        }),
    };
    if let Some(supported) = supported_architectures {
        host.supported_architectures = Some(supported.clone());
    }
    Some(host)
}

/// Resolve the engine name that keys the install's store slots.
///
/// Priority mirrors the frozen path: a `node@runtime:` pin in the
/// lockfile wins outright (so pinned and non-pinned installs on the same
/// host share the store), then the already-detected host, then a `node
/// --version` probe. The probe is returned as a still-running
/// [`JoinHandle`][tokio::task::JoinHandle] so it overlaps
/// `CreateVirtualStore`'s I/O — except under the global virtual store,
/// whose layout needs the name synchronously.
pub(super) async fn resolve_engine_name(
    config: &Config,
    lockfile: &Lockfile,
    host_node: Option<&(bool, String)>,
) -> (Option<String>, Option<tokio::task::JoinHandle<Option<String>>>) {
    fn probe() -> Option<String> {
        pacquet_graph_hasher::detect_node_major()
            .map(|major| pacquet_graph_hasher::engine_name(major, None, None))
    }

    if let Some(major) =
        crate::install_frozen_lockfile::find_runtime_node_major(lockfile.snapshots.as_ref())
    {
        return (Some(pacquet_graph_hasher::engine_name(major, None, None)), None);
    }
    match host_node {
        Some((true, version)) => (
            crate::install_frozen_lockfile::parse_major_from_version(version)
                .map(|major| pacquet_graph_hasher::engine_name(major, None, None)),
            None,
        ),
        Some((false, _)) => (None, None),
        None if config.enable_global_virtual_store => {
            (tokio::task::spawn_blocking(probe).await.ok().flatten(), None)
        }
        None => (None, Some(tokio::task::spawn_blocking(probe))),
    }
}

pub(super) struct SkipSetInputs<'a> {
    pub requester: &'a str,
    /// The lockfile the installability pass evaluates: the
    /// materialization closure under a filtered install, the full built
    /// lockfile otherwise.
    pub materialization_lockfile: &'a Lockfile,
    /// The full built lockfile, whose importers anchor the reachability
    /// closure.
    pub built_lockfile: &'a Lockfile,
    pub lockfile_dir: &'a Path,
    /// `None` when installability checks are bypassed — see
    /// [`detect_installability_host`].
    pub installability_host: Option<&'a crate::InstallabilityHost>,
    pub included: IncludedDependencies,
    pub dependency_groups: &'a [DependencyGroup],
    /// See [`super::InstallWithFreshLockfile::is_full_install`].
    pub is_full_install: bool,
    /// See [`super::InstallWithFreshLockfile::skip_runtimes`].
    pub skip_runtimes: bool,
}

/// Compute the snapshots this install must not materialize: the
/// installability skips, the `--no-optional` exclusions, the
/// `--no-runtime` direct-runtime skips, and the reachability closure over
/// all of them.
pub(super) fn compute_skip_set<Reporter: pacquet_reporter::Reporter>(
    inputs: &SkipSetInputs<'_>,
) -> Result<SkippedSnapshots, InstallWithFreshLockfileError> {
    let &SkipSetInputs {
        requester,
        materialization_lockfile,
        built_lockfile,
        lockfile_dir,
        installability_host,
        included,
        dependency_groups,
        is_full_install,
        skip_runtimes,
    } = inputs;

    let mut skipped = match (
        materialization_lockfile.snapshots.as_ref(),
        materialization_lockfile.packages.as_ref(),
        installability_host,
    ) {
        (Some(snapshots), Some(packages), Some(host)) => {
            crate::compute_skipped_snapshots::<Reporter>(
                &materialization_lockfile.importers,
                snapshots,
                packages,
                host,
                requester,
                SkippedSnapshots::new(),
            )
            .map_err(InstallWithFreshLockfileError::Installability)?
        }
        _ => SkippedSnapshots::new(),
    };

    // `--no-optional` excludes the Optional group. `dependency_groups`
    // already drops the root importer's own optional direct deps, but a
    // transitive optional (an `optionalDependencies` entry of a resolved
    // package) is still in the graph and would be materialized. Exclude
    // every optional-only snapshot the same way the frozen-lockfile path
    // does — via the transient `optional_excluded` skip set, which keeps
    // it out of materialization and `.modules.yaml.skipped` yet leaves it
    // in the lockfile, so a later install without the flag restores it.
    //
    // Gated on `is_full_install`: only a full install's
    // `dependency_groups` carries a `--no-optional` intent. Partial runs
    // either pass every direct group (`add`, `remove`, `update`) or
    // narrow the groups for reasons of their own (`fetch --dev`,
    // `rebuild`), and must keep their transitive optionals (e.g.
    // `@pnpm/exe`'s platform binary on the engine install).
    if is_full_install
        && !dependency_groups.contains(&DependencyGroup::Optional)
        && let Some(snapshots) = materialization_lockfile.snapshots.as_ref()
    {
        for (key, snapshot) in snapshots {
            if snapshot.optional {
                skipped.add_optional_excluded(key.clone());
            }
        }
    }

    if skip_runtimes && let Some(packages) = materialization_lockfile.packages.as_ref() {
        crate::add_direct_runtime_skips(
            &mut skipped,
            &materialization_lockfile.importers,
            packages,
        );
    }

    // The recorded skip set must be the reachability closure of the
    // direct skips (see [`crate::extend_skipped_with_dependency_closure`]);
    // extend it before the materialization closure, hoist, symlink, and
    // bin passes consume it.
    let importer_ids: std::collections::HashSet<String> =
        built_lockfile.importers.keys().cloned().collect();
    crate::extend_skipped_with_dependency_closure(
        &mut skipped,
        built_lockfile,
        lockfile_dir,
        &importer_ids,
        included,
    );

    Ok(skipped)
}
