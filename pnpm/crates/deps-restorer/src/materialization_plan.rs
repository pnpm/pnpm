//! What an install is allowed to put on disk: the host its constraints
//! are checked against, the set of snapshots it must not materialize,
//! and the engine name its store slots are keyed by.
//!
//! Shared by both install paths. They agree on every step here — the
//! skip set decides what lands in `node_modules`, so the two paths
//! producing it differently would be a correctness bug rather than a
//! stylistic difference. The two places they legitimately differ are
//! parameters: [`SkipSetInputs::seed`] and
//! [`SkipSetInputs::exclude_optional`].

use crate::{
    InstallabilityHost, SkippedSnapshots, add_direct_runtime_skips, compute_skipped_snapshots,
    extend_skipped_with_dependency_closure,
    install_frozen_lockfile::{find_runtime_node_major, parse_major_from_version},
};
use pnpm_lockfile::{Lockfile, PackageKey, PackageMetadata, ProjectSnapshot, SnapshotEntry};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_is_installable::{InstallabilityError, SupportedArchitectures};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// The host Node, reduced to what the phases after the installability
/// check still need.
#[derive(Debug, Clone)]
pub struct HostNode {
    pub version: String,
    /// `false` when detection fell back to the synthetic host. A
    /// synthetic version must not key the store: it would poison both
    /// the side-effects cache and the global-virtual-store hash.
    pub detected: bool,
}

impl From<&InstallabilityHost> for HostNode {
    fn from(host: &InstallabilityHost) -> Self {
        HostNode { version: host.node_version.clone(), detected: host.node_detected }
    }
}

/// Detect the host an install's `os` / `cpu` / `libc` / `engines`
/// constraints are checked against.
///
/// Whether detection is `needed` stays with the caller: the two install
/// paths qualify it differently, and getting it wrong is not obvious
/// from here. Passing `false` returns `None`, which is what keeps `node
/// --version` off the critical path of the common unconstrained install
/// — the probe would otherwise serialize against the extraction that
/// dominates a cold install.
pub async fn detect_installability_host(
    needed: bool,
    engine_strict: bool,
    node_version: Option<String>,
    supported_architectures: Option<&SupportedArchitectures>,
) -> Option<InstallabilityHost> {
    if !needed {
        return None;
    }

    let mut host = match node_version {
        // An explicit `nodeVersion` needs no `node --version` probe, so
        // build the host directly off the reactor thread.
        node_version @ Some(_) => InstallabilityHost::detect_with(engine_strict, node_version),
        None => tokio::task::spawn_blocking(move || {
            InstallabilityHost::detect_with(engine_strict, None)
        })
        .await
        .unwrap_or_else(|_| InstallabilityHost {
            node_version: "99999.0.0".to_string(),
            node_detected: false,
            os: pnpm_graph_hasher::host_platform(),
            cpu: pnpm_graph_hasher::host_arch(),
            libc: pnpm_graph_hasher::host_libc(),
            supported_architectures: None,
            engine_strict,
        }),
    };
    // Plant the CLI-merged `supportedArchitectures` (yaml +
    // `--cpu`/`--os`/`--libc`) onto the host so `check_platform`'s
    // `dedupe_current` substitution picks up user-supplied accept lists
    // rather than only the host triple.
    if let Some(supported) = supported_architectures {
        host.supported_architectures = Some(supported.clone());
    }
    Some(host)
}

pub struct SkipSetInputs<'a> {
    pub requester: &'a str,
    /// Importers the installability pass evaluates against.
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    /// The snapshots and metadata being evaluated. Under a filtered
    /// install these are the materialization closure's, not the whole
    /// lockfile's.
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// `None` when installability checks are bypassed — see
    /// [`detect_installability_host`].
    pub installability_host: Option<&'a InstallabilityHost>,
    /// Skips carried in from the previous install's
    /// `.modules.yaml.skipped`, so a package already known to be
    /// incompatible is not re-checked and does not re-emit
    /// `pnpm:skipped-optional-dependency`.
    ///
    /// The frozen path seeds this; the fresh path starts empty, because
    /// it has just re-resolved the graph and cannot assume the previous
    /// run's verdicts still apply.
    pub seed: SkippedSnapshots,
    /// Whether to drop every `optional: true` snapshot.
    ///
    /// Both paths mean `--no-optional` by this, but they qualify it
    /// differently: only a *full* install's dependency groups carry that
    /// intent, so the fresh path additionally requires
    /// `is_full_install`. Deciding it in the caller keeps that
    /// distinction visible where the reason for it lives.
    pub exclude_optional: bool,
    pub skip_runtimes: bool,
    /// The lockfile whose importers anchor the reachability closure —
    /// the full one, even under a filtered install.
    pub closure_lockfile: &'a Lockfile,
    pub closure_root: &'a Path,
    pub closure_importer_ids: &'a HashSet<String>,
    pub included: IncludedDependencies,
}

/// Compute the snapshots this install must not materialize: the
/// installability skips, the `--no-optional` exclusions, the
/// `--no-runtime` direct-runtime skips, and the reachability closure
/// over all of them.
///
/// The closure runs last and is not optional: every downstream consumer
/// (`CreateVirtualStore`, the hoist pass, the symlink and bin passes)
/// reads the finished set, and a skip that is not closed over leaves
/// dangling links to a package that was never placed.
pub fn compute_skip_set<Reporter: pnpm_reporter::Reporter>(
    inputs: SkipSetInputs<'_>,
) -> Result<SkippedSnapshots, Box<InstallabilityError>> {
    let SkipSetInputs {
        requester,
        importers,
        snapshots,
        packages,
        installability_host,
        seed,
        exclude_optional,
        skip_runtimes,
        closure_lockfile,
        closure_root,
        closure_importer_ids,
        included,
    } = inputs;

    let mut skipped = match (snapshots, packages, installability_host) {
        (Some(snapshots), Some(packages), Some(host)) => compute_skipped_snapshots::<Reporter>(
            importers, snapshots, packages, host, requester, seed,
        )?,
        // Constraint-free lockfile: keep the seed verbatim, so a
        // snapshot recorded as skipped previously survives the
        // constraint having since been removed from the lockfile.
        _ => seed,
    };

    // The lockfile's `optional` flag is set only when a snapshot is
    // reachable *exclusively* through optional edges, so a dependency
    // that is both optional and required still survives this filter.
    // These land in the transient `optional_excluded` subset: excluded
    // from materialization, but kept out of `.modules.yaml.skipped` so a
    // later install without the flag brings them back.
    if exclude_optional && let Some(snapshots) = snapshots {
        for (key, snapshot) in snapshots {
            if snapshot.optional {
                skipped.add_optional_excluded(key.clone());
            }
        }
    }

    if skip_runtimes && let Some(packages) = packages {
        add_direct_runtime_skips(&mut skipped, importers, packages);
    }

    extend_skipped_with_dependency_closure(
        &mut skipped,
        closure_lockfile,
        closure_root,
        closure_importer_ids,
        included,
    );

    Ok(skipped)
}

/// Resolve the engine name that keys the install's store slots and the
/// side-effects-cache prefix.
///
/// A `node@runtime:` pin in the lockfile wins outright, so pinned and
/// non-pinned installs on the same host share one store rather than
/// splitting it under whatever `node --version` the shell reports. Then
/// the already-detected host, then a probe.
///
/// The probe comes back as a still-running
/// [`JoinHandle`][tokio::task::JoinHandle] so it overlaps the
/// virtual-store I/O — except under the global virtual store, whose
/// layout needs the name synchronously.
pub async fn resolve_engine_name(
    enable_global_virtual_store: bool,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    host_node: Option<&HostNode>,
) -> (Option<String>, Option<tokio::task::JoinHandle<Option<String>>>) {
    fn probe() -> Option<String> {
        pnpm_graph_hasher::detect_node_major()
            .map(|major| pnpm_graph_hasher::engine_name(major, None, None))
    }

    if let Some(major) = find_runtime_node_major(snapshots) {
        return (Some(pnpm_graph_hasher::engine_name(major, None, None)), None);
    }
    match host_node {
        Some(HostNode { version, detected: true }) => (
            parse_major_from_version(version)
                .map(|major| pnpm_graph_hasher::engine_name(major, None, None)),
            None,
        ),
        Some(HostNode { detected: false, .. }) => (None, None),
        None if enable_global_virtual_store => {
            (tokio::task::spawn_blocking(probe).await.ok().flatten(), None)
        }
        None => (None, Some(tokio::task::spawn_blocking(probe))),
    }
}
