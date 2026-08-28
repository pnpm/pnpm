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
    sync::{Arc, OnceLock},
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

/// A host detection that is either done or still probing on a spawned
/// task. The frozen install path spawns the detection early and only
/// resolves it once the skip set needs the host, so the `node
/// --version` inside overlaps the store-side warm-cache prefetch
/// instead of serializing before it.
pub enum HostDetection {
    Resolved(Option<InstallabilityHost>),
    Pending {
        task: tokio::task::JoinHandle<Option<InstallabilityHost>>,
        /// Carried so a joined-task failure can synthesize the same
        /// fallback host [`detect_installability_host`] would have,
        /// CLI-merged `supportedArchitectures` accept lists included.
        engine_strict: bool,
        supported_architectures: Option<SupportedArchitectures>,
    },
}

impl HostDetection {
    /// Spawn the detection so it runs under whatever the caller does
    /// next. The earlier this is called, the more of the probe's `node`
    /// startup hides — the install entry point spawns one right after
    /// the wanted lockfile parses, when the lockfile carries a
    /// constraint that will need the host.
    #[must_use]
    pub fn spawn(
        engine_strict: bool,
        node_version: Option<String>,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> Self {
        HostDetection::Pending {
            task: tokio::spawn({
                let supported_architectures = supported_architectures.clone();
                async move {
                    detect_installability_host(
                        true,
                        engine_strict,
                        node_version,
                        supported_architectures.as_ref(),
                    )
                    .await
                }
            }),
            engine_strict,
            supported_architectures,
        }
    }

    /// Wait for the detection. A joined-task failure degrades to the
    /// synthetic fallback host, so the installability checks still run
    /// — the same degradation [`detect_installability_host`] applies
    /// when its own probe task fails.
    pub async fn resolve(self) -> Option<InstallabilityHost> {
        match self {
            HostDetection::Resolved(host) => host,
            HostDetection::Pending { task, engine_strict, supported_architectures } => {
                task.await.unwrap_or_else(|error| {
                    tracing::warn!(
                        target: "pacquet::install",
                        ?error,
                        "host detection task failed; falling back to the synthetic host",
                    );
                    let mut host = synthetic_installability_host(engine_strict);
                    host.supported_architectures = supported_architectures;
                    Some(host)
                })
            }
        }
    }
}

/// The stand-in host used when `node --version` cannot be probed. Its
/// `node_detected: false` keeps the bogus version out of the store keys
/// (see [`engine_name_from_host`]) while the `os` / `cpu` / `libc`
/// platform checks still run against the real host triple.
fn synthetic_installability_host(engine_strict: bool) -> InstallabilityHost {
    InstallabilityHost {
        node_version: "99999.0.0".to_string(),
        node_detected: false,
        os: pnpm_graph_hasher::host_platform(),
        cpu: pnpm_graph_hasher::host_arch(),
        libc: pnpm_graph_hasher::host_libc(),
        supported_architectures: None,
        engine_strict,
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
        .unwrap_or_else(|_| synthetic_installability_host(engine_strict)),
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

/// A `node --version` probe still in flight. See
/// [`resolve_engine_name`] for when one is spawned.
///
/// The result is delivered twice: [`Self::handle`] for the async
/// consumer (the side-effects-cache key in the build phase, awaited
/// after linking), and a shared slot for a synchronous consumer that
/// needs the name from a worker thread before the handle is awaited —
/// the directory-clone cache's lazily built layout (see
/// [`crate::DirCloneCache`]).
pub struct DeferredEngineName {
    pub handle: tokio::task::JoinHandle<Option<String>>,
    shared: Arc<OnceLock<Option<String>>>,
}

impl DeferredEngineName {
    fn spawn() -> Self {
        /// Fills the slot with `None` when dropped: a synchronous
        /// consumer blocked in [`OnceLock::wait`] would otherwise sleep
        /// forever if the probe panicked — or never ran at all, which
        /// is why the guard is captured by the closure rather than
        /// created inside it: a shutting-down runtime that discards the
        /// still-queued closure drops its environment, and the guard
        /// with it.
        struct FillOnDrop(Arc<OnceLock<Option<String>>>);
        impl Drop for FillOnDrop {
            fn drop(&mut self) {
                let _ = self.0.set(None);
            }
        }

        let shared = Arc::new(OnceLock::new());
        let handle = tokio::task::spawn_blocking({
            let guard = FillOnDrop(Arc::clone(&shared));
            let shared = Arc::clone(&shared);
            move || {
                let name = probe_engine_name();
                let _ = shared.set(name.clone());
                drop(guard);
                name
            }
        });
        DeferredEngineName { handle, shared }
    }

    /// The slot the probe fills on completion. Blocking on it via
    /// [`OnceLock::wait`] must happen off the async reactor.
    #[must_use]
    pub fn shared(&self) -> Arc<OnceLock<Option<String>>> {
        Arc::clone(&self.shared)
    }
}

fn probe_engine_name() -> Option<String> {
    pnpm_graph_hasher::detect_node_major()
        .map(|major| pnpm_graph_hasher::engine_name(major, None, None))
}

/// The engine name a detected host implies. `None` for the synthetic
/// fallback host: a bogus `99999.0.0`-derived key must not poison the
/// side-effects cache or the GVS hash.
#[must_use]
pub fn engine_name_from_host(host_node: &HostNode) -> Option<String> {
    if !host_node.detected {
        return None;
    }
    parse_major_from_version(&host_node.version)
        .map(|major| pnpm_graph_hasher::engine_name(major, None, None))
}

/// The engine name a lockfile `node@runtime:` pin implies, when one is
/// present. Both engine-resolution paths — [`resolve_engine_name`] and
/// the frozen path's deferred-host branch — apply this one rule, so
/// they can't drift apart on how a pin keys the store.
#[must_use]
pub fn engine_name_from_runtime_pin(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> Option<String> {
    find_runtime_node_major(snapshots)
        .map(|major| pnpm_graph_hasher::engine_name(major, None, None))
}

/// Resolve the engine name that keys the install's store slots and the
/// side-effects-cache prefix.
///
/// A `node@runtime:` pin in the lockfile wins outright, so pinned and
/// non-pinned installs on the same host share one store rather than
/// splitting it under whatever `node --version` the shell reports. Then
/// the already-detected host, then a probe.
///
/// The probe comes back as a still-running [`DeferredEngineName`] so it
/// overlaps the virtual-store I/O — except under the global virtual
/// store, whose layout needs the name synchronously.
pub async fn resolve_engine_name(
    enable_global_virtual_store: bool,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    host_node: Option<&HostNode>,
) -> (Option<String>, Option<DeferredEngineName>) {
    if let Some(name) = engine_name_from_runtime_pin(snapshots) {
        return (Some(name), None);
    }
    match host_node {
        Some(host_node @ HostNode { detected: true, .. }) => {
            (engine_name_from_host(host_node), None)
        }
        Some(HostNode { detected: false, .. }) => (None, None),
        None if enable_global_virtual_store => {
            (tokio::task::spawn_blocking(probe_engine_name).await.ok().flatten(), None)
        }
        None => (None, Some(DeferredEngineName::spawn())),
    }
}
