//! Per-install installability pass.
//!
//! For each snapshot in a frozen-lockfile install, run
//! `pacquet-package-is-installable`'s `check_package` against the
//! matching `PackageMetadata` and the host environment, build the
//! [`SkippedSnapshots`] set, and emit
//! `pnpm:skipped-optional-dependency` for every optional+incompatible
//! one.
//!
//! Mirrors the union of upstream's:
//! - The resolver-side gate at
//!   <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-resolver/src/resolveDependencies.ts#L1307-L1312>.
//! - The headless re-check at
//!   <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L206-L215>.
//!
//! Pacquet's install path is lockfile-driven and has no resolver, so
//! the headless re-check is the only relevant emit site. Running it
//! every install also means the set is recomputed against the current
//! host — pnpm's `lockfileToDepGraph` does exactly the same, and the
//! comment at upstream's `:194-215` calls out that the host arch may
//! have changed since the previous install wrote `.modules.yaml`.

use std::collections::{HashMap, HashSet};

use pacquet_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pacquet_package_is_installable::{
    InstallabilityError, InstallabilityOptions, PackageInstallabilityManifest, SkipReason,
    SupportedArchitectures, WantedEngine, WantedPlatformRef, check_package, inferred_platform,
};
use pacquet_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};

/// The set of snapshot keys skipped on this host.
///
/// Three disjoint origin classes are tracked separately because
/// they behave differently across installs:
///
/// - **Installability skips** (`installability`) — engine, platform,
///   or libc mismatch surfaced by [`compute_skipped_snapshots`].
///   Persisted to `.modules.yaml.skipped` and re-seeded on every
///   subsequent install, mirroring upstream's
///   `opts.skipped.add(depPath)` at
///   <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L213>.
///
/// - **Fetch-failure skips** (`fetch_failed`) — an `optional: true`
///   snapshot whose tarball / metadata / extract step blew up
///   during the install. **Not** persisted, matching upstream's
///   silent `if (pkgSnapshot.optional) return` at
///   <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L294-L298>:
///   upstream's catch site never updates `opts.skipped`, so a
///   subsequent install retries the fetch.
///
/// - **`--no-optional` exclusions** (`optional_excluded`) —
///   snapshots whose lockfile entry has `optional: true` AND the
///   user passed `--no-optional` (or `IncludedDependencies::optional_dependencies`
///   is false). **Not** persisted, matching upstream's behavior:
///   the filter sits at
///   <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/link.ts#L109-L111>,
///   downstream of the `opts.skipped` set, so re-running without
///   `--no-optional` brings the snapshots back into the install
///   graph. Pacquet's downstream architecture walks the lockfile
///   directly rather than a pre-pruned graph, so a separate filter
///   is needed where upstream gets it for free from the depNode
///   filter chain.
///
/// All three subsets contribute to [`contains`] and [`iter`] —
/// downstream walkers treat skipped-for-any-reason uniformly. Only
/// the `installability` subset survives [`iter_installability`],
/// which is what `.modules.yaml.skipped` writes.
///
/// [`compute_skipped_snapshots`]: crate::compute_skipped_snapshots
/// [`contains`]: SkippedSnapshots::contains
/// [`iter`]: SkippedSnapshots::iter
/// [`iter_installability`]: SkippedSnapshots::iter_installability
#[derive(Debug, Default, Clone)]
pub struct SkippedSnapshots {
    installability: HashSet<PackageKey>,
    fetch_failed: HashSet<PackageKey>,
    optional_excluded: HashSet<PackageKey>,
}

impl SkippedSnapshots {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a [`SkippedSnapshots`] from an existing
    /// installability set. Test helper for callers that want to
    /// drive build-sequence / virtual-store filtering against a
    /// known skip set without running the full installability pass.
    #[cfg(test)]
    pub(crate) fn from_set(set: HashSet<PackageKey>) -> Self {
        Self { installability: set, ..Self::default() }
    }

    /// Seed the installability set with snapshot keys recorded as
    /// skipped by a previous install (read from
    /// `.modules.yaml.skipped`). Unparsable strings are silently
    /// dropped — upstream tolerates the same shape mismatch at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L194>
    /// (the seed is only consulted by `Set.has(depPath)`; a
    /// nonsense string never matches any current snapshot, so the
    /// orphan is harmless).
    pub fn from_strings<Iter>(iter: Iter) -> Self
    where
        Iter: IntoIterator,
        Iter::Item: AsRef<str>,
    {
        let installability =
            iter.into_iter().filter_map(|text| text.as_ref().parse::<PackageKey>().ok()).collect();
        Self { installability, ..Self::default() }
    }

    /// Record an `optional: true` snapshot whose fetch / extract
    /// failed during this install. Slice 4 wire-up — call site is
    /// inside [`crate::CreateVirtualStore`]'s cold-batch dispatch.
    ///
    /// Disjoint-subset guard: if `key` is already in any other
    /// subset, the insert is a no-op so [`len`] / [`iter`] stay
    /// consistent with [`contains`]. In practice the only
    /// realistic overlap is with `installability`
    /// (`optional_excluded` snapshots are dropped before reaching
    /// the cold-batch dispatch), but the guard is symmetric with
    /// [`add_optional_excluded`]'s so the public API enforces the
    /// invariant regardless of call order.
    ///
    /// [`len`]: SkippedSnapshots::len
    /// [`iter`]: SkippedSnapshots::iter
    /// [`contains`]: SkippedSnapshots::contains
    /// [`add_optional_excluded`]: SkippedSnapshots::add_optional_excluded
    pub fn add_fetch_failed(&mut self, key: PackageKey) {
        if self.installability.contains(&key) || self.optional_excluded.contains(&key) {
            return;
        }
        self.fetch_failed.insert(key);
    }

    /// Record a snapshot dropped because the user passed
    /// `--no-optional` (or the matching config / `IncludedDependencies`
    /// flag is false). Slice 5 wire-up — call site is inside
    /// `InstallFrozenLockfile::run`, which iterates the lockfile
    /// snapshots once and inserts every `snap.optional == true`
    /// entry. Downstream gates then drop the snapshot from
    /// extraction, symlinking, building, and hoisting through the
    /// same skip-set check they use for installability skips.
    ///
    /// Disjoint-subset guard: a snapshot that is both
    /// installability-skipped (platform / engine mismatch) and
    /// would-be excluded by `--no-optional` stays in the
    /// higher-precedence `installability` subset only. This is the
    /// realistic overlap case (an `optional: true` snapshot that's
    /// also `os: [<wrong>]`), and putting it in both subsets would
    /// make [`len`] / [`iter`] inconsistent with [`contains`].
    /// Same guard applies against `fetch_failed`, though that
    /// overlap can't arise in practice (a snapshot dropped by
    /// `--no-optional` never reaches the cold-batch dispatch).
    ///
    /// [`len`]: SkippedSnapshots::len
    /// [`iter`]: SkippedSnapshots::iter
    /// [`contains`]: SkippedSnapshots::contains
    pub fn add_optional_excluded(&mut self, key: PackageKey) {
        if self.installability.contains(&key) || self.fetch_failed.contains(&key) {
            return;
        }
        self.optional_excluded.insert(key);
    }

    /// `true` if the snapshot is skipped for **any** reason
    /// (installability, fetch-failure, or `--no-optional`).
    /// Downstream consumers want the union: a dropped snapshot is
    /// equally absent from the install regardless of origin.
    #[must_use]
    pub fn contains(&self, key: &PackageKey) -> bool {
        self.installability.contains(key)
            || self.fetch_failed.contains(key)
            || self.optional_excluded.contains(key)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.installability.len() + self.fetch_failed.len() + self.optional_excluded.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.installability.is_empty()
            && self.fetch_failed.is_empty()
            && self.optional_excluded.is_empty()
    }

    /// Insert into the installability set. Used by
    /// [`compute_skipped_snapshots`] when the per-snapshot
    /// installability check fails.
    pub(crate) fn insert_installability(&mut self, key: PackageKey) {
        self.installability.insert(key);
    }

    /// Iterate over the **installability** subset only — the entries
    /// written to `.modules.yaml.skipped`. Fetch-failure and
    /// `--no-optional` entries are transient and intentionally
    /// excluded so they aren't persisted across installs.
    pub fn iter_installability(&self) -> impl Iterator<Item = &PackageKey> + '_ {
        self.installability.iter()
    }

    /// Iterate over the union of all subsets — every snapshot that
    /// downstream consumers should treat as absent from the install,
    /// regardless of origin. Used by `hoist.rs` and similar
    /// graph-walking passes that don't care why a snapshot is gone.
    pub fn iter(&self) -> impl Iterator<Item = &PackageKey> + '_ {
        self.installability
            .iter()
            .chain(self.fetch_failed.iter())
            .chain(self.optional_excluded.iter())
    }
}

/// Host context for the installability check. Built once per install
/// so the per-snapshot calls don't each re-spawn `node --version`
/// or re-read `std::env::consts::OS`.
pub struct InstallabilityHost {
    pub node_version: String,
    /// `true` when `node_version` was discovered by spawning
    /// `node --version`; `false` when the field carries the synthetic
    /// fallback. The side-effects-cache key derives from this — a
    /// fallback version must not seed the cache because subsequent
    /// installs would key on the actual node major and miss every
    /// row written under the fallback.
    pub node_detected: bool,
    pub os: &'static str,
    pub cpu: &'static str,
    pub libc: &'static str,
    pub supported_architectures: Option<SupportedArchitectures>,
    pub engine_strict: bool,
}

impl InstallabilityHost {
    /// Resolve the host context from the running process.
    ///
    /// `node_version` is detected via
    /// [`pacquet_graph_hasher::detect_node_version`]; when detection
    /// fails (no `node` on PATH), pacquet falls back to a synthetic
    /// `99999.0.0` so `engines.node` ranges keep accepting packages.
    /// The alternative `0.0.0` would falsely-skip every optional
    /// dependency targeting any concrete node range, which is worse
    /// than the over-acceptance the very-high fallback produces.
    /// `node_detected` records which path was taken so callers can
    /// suppress side-effects-cache lookups when the version is
    /// synthetic. Slice 2 will wire a proper `nodeVersion` config
    /// setting and surface `ERR_PNPM_INVALID_NODE_VERSION` to match
    /// upstream's throw-on-detection-failure behavior.
    #[must_use]
    pub fn detect() -> Self {
        let detected = pacquet_graph_hasher::detect_node_version();
        let node_detected = detected.is_some();
        let node_version = detected.unwrap_or_else(|| "99999.0.0".to_string());
        Self {
            node_version,
            node_detected,
            os: pacquet_graph_hasher::host_platform(),
            cpu: pacquet_graph_hasher::host_arch(),
            libc: pacquet_graph_hasher::host_libc(),
            supported_architectures: None,
            engine_strict: false,
        }
    }
}

/// Compute the [`SkippedSnapshots`] set for a frozen-lockfile install.
///
/// For each `(snapshot_key, snapshot)`:
/// 1. Look up the matching `PackageMetadata` (skipping snapshots
///    without one — `CreateVirtualStore` will error on them
///    separately).
/// 2. Build a [`PackageInstallabilityManifest`] from `metadata.engines`,
///    `metadata.cpu`, `metadata.os`, `metadata.libc`.
/// 3. Run `check_package` against the host triple.
/// 4. Apply the per-snapshot dispatch:
///    - `Ok(None)`: compatible, nothing to do.
///    - `Ok(Some(err))` + `snapshot.optional`: add to the set; emit
///      `pnpm:skipped-optional-dependency`.
///    - `Ok(Some(err))` + `engine_strict`: return as the install
///      error. Pacquet's default has `engine_strict = false`, so
///      this path is currently unreachable from production — wired
///      for the slice that lands the config setting.
///    - `Ok(Some(err))` otherwise: emit `tracing::warn!` and proceed.
///      Upstream uses `pnpm:install-check` here, which pacquet's
///      reporter does not yet expose — slice 1 follow-up.
///    - `Err(InvalidNodeVersionError)`: surface as
///      `ERR_PNPM_INVALID_NODE_VERSION`.
pub fn compute_skipped_snapshots<Reporter: self::Reporter>(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    host: &InstallabilityHost,
    prefix: &str,
    seed: SkippedSnapshots,
) -> Result<SkippedSnapshots, Box<InstallabilityError>> {
    // Fast path: if no package in the lockfile declares any
    // installability constraint, every snapshot is trivially
    // installable. Skip the per-snapshot
    // `without_peer()` / `to_string()` / `check_package` loop
    // entirely. Pacquet has no resolver so the lockfile's packages
    // map is fixed for the duration of the install; one linear scan
    // early is much cheaper than walking the snapshots map and
    // decomposing each metadata row only to find no constraints to
    // evaluate.
    //
    // The `seed` is returned as-is on the fast path so previously
    // skipped snapshots survive across reinstalls even when the
    // lockfile's per-snapshot constraints have since been removed.
    // Mirrors upstream's early-return behavior at
    // <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L194>.
    //
    // Concretely on the integrated benchmark (1352 packages with no
    // platform / engine constraints): drops ~1352 `String` and
    // `PackageKey` allocations and the matching number of
    // `check_package` calls. The scan is O(N) on `packages` — same
    // shape as the loop it short-circuits — but does at most four
    // `Option::is_some` checks per row and short-circuits on the
    // first declared constraint.
    if !any_installability_constraint(snapshots, packages) {
        return Ok(seed);
    }

    let mut skipped = seed;
    let mut seen_emit: HashSet<PackageKey> = HashSet::new();

    // Build the host-derived part of the options once. Only the
    // (`engine_strict`-irrelevant) `optional` flag varies per
    // snapshot, but the result of [`check_package`] — "does this
    // manifest satisfy the host?" — does not. We compute and cache
    // the check verdict per peer-stripped `metadata_key`; the
    // per-snapshot loop then only needs to apply the
    // optional / engine-strict dispatch.
    //
    // The cache pays off on lockfiles with peer-resolved variants of
    // the same package (`react-dom@17(react@17)` /
    // `react-dom@17(react@18)`, etc.) — every variant shares the
    // same `metadata_key`, so the check only runs once.
    // `InstallabilityOptions` borrows its string fields for exactly
    // this reuse pattern.
    let base_options = InstallabilityOptions {
        engine_strict: host.engine_strict,
        // Cache-shared check: `optional` is applied per-snapshot
        // below, not inside `check_package`.
        optional: false,
        current_node_version: host.node_version.as_str(),
        pnpm_version: None,
        current_os: host.os,
        current_cpu: host.cpu,
        current_libc: host.libc,
        supported_architectures: host.supported_architectures.as_ref(),
    };

    // `None` = compatible. `Some(err)` = incompatible, with the
    // diagnostic the caller would surface (used as both the
    // `SkipOptional` details payload and the `ProceedWithWarning`
    // message body, matching upstream's `warn.toString()` / `warn.message`
    // at `index.ts:50` / `:44`).
    //
    // The key carries the snapshot's `optional` flag because the
    // platform-from-name inference only runs for optional snapshots,
    // so the verdict of an optional and a non-optional snapshot of
    // the same metadata row can differ.
    let mut check_cache: HashMap<(PackageKey, bool), Option<InstallabilityError>> = HashMap::new();

    for (snapshot_key, snapshot) in snapshots {
        // Seeded entries short-circuit the per-snapshot re-check.
        // Mirrors upstream's `if (opts.skipped.has(depPath)) return`
        // at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L194>:
        // a snapshot recorded as skipped on the previous install is
        // not re-evaluated and emits no
        // `pnpm:skipped-optional-dependency` event, so the user is
        // not re-notified of a known skip on every reinstall.
        if skipped.contains(snapshot_key) {
            continue;
        }

        let metadata_key = snapshot_key.without_peer();
        let Some(metadata) = packages.get(&metadata_key) else { continue };

        // Cache miss → run `check_package` once for this metadata
        // row. The clone-on-insert is a single `Option<InstallabilityError>`
        // (small) and only happens on the first peer-variant of each
        // package. Subsequent peer-variants land in the `else` arm
        // and read back the cached verdict.
        let cache_key = (metadata_key.clone(), snapshot.optional);
        let warn = if let Some(cached) = check_cache.get(&cache_key) {
            cached.clone()
        } else {
            let mut manifest = manifest_from_metadata(&metadata_key, metadata);
            // Mirrors upstream's `effectivePlatform(pkg, options.optional)` at
            // <https://github.com/pnpm/pnpm/blob/34875b2d7c/config/package-is-installable/src/index.ts#L41>:
            // an optional snapshot with incomplete platform fields gets
            // the missing ones filled from the package name.
            if snapshot.optional
                && let Some(platform) = inferred_platform(
                    &manifest.name,
                    WantedPlatformRef {
                        os: manifest.os.as_deref(),
                        cpu: manifest.cpu.as_deref(),
                        libc: manifest.libc.as_deref(),
                    },
                )
            {
                manifest.os = platform.os;
                manifest.cpu = platform.cpu;
                manifest.libc = platform.libc;
            }
            let pkg_id = metadata_key.to_string();
            let result = check_package(&pkg_id, &manifest, &base_options)
                .map_err(|invalid| Box::new(InstallabilityError::InvalidNodeVersion(invalid)))?;
            check_cache.insert(cache_key, result.clone());
            result
        };

        let Some(warn) = warn else { continue };

        if snapshot.optional {
            skipped.insert_installability(snapshot_key.clone());
            // Dedup events per metadata key, matching upstream's
            // emit-per-pkgId at `index.ts:49-58`.
            if seen_emit.insert(metadata_key.clone()) {
                emit_skipped::<Reporter>(
                    &metadata_key.to_string(),
                    warn.skip_reason(),
                    warn.to_string(),
                    prefix,
                );
            }
            continue;
        }

        if host.engine_strict {
            return Err(Box::new(warn));
        }

        // Non-optional, non-strict: upstream emits `pnpm:install-check`
        // warn (TODO: add channel to the reporter). For now the
        // tracing-level warning is the user-visible signal that an
        // incompatible non-optional dep slipped through.
        tracing::warn!(
            target: "pacquet::install",
            package = %metadata_key,
            "{}",
            warn,
        );
    }

    Ok(skipped)
}

/// True if any package metadata row in the lockfile declares an
/// `engines` / `cpu` / `os` / `libc` constraint pacquet would need
/// to evaluate, or any optional snapshot's package name infers a
/// platform constraint its metadata row doesn't declare.
/// Short-circuits on the first hit. When this returns false, both
/// [`compute_skipped_snapshots`] and the caller can short-circuit:
/// no need to spawn `node --version` or build the host context,
/// because the verdict is unconditionally an empty skip set.
///
/// `pub` so `install_frozen_lockfile` can gate the host detection
/// on it — the spawn is otherwise on the critical path of
/// `CreateVirtualStore::run` and serializes ~100ms of node-binary
/// startup with extraction it used to overlap with.
pub fn any_installability_constraint(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> bool {
    packages.values().any(metadata_has_meaningful_constraint)
        || snapshots.iter().any(|(snapshot_key, snapshot)| {
            snapshot.optional && {
                let metadata_key = snapshot_key.without_peer();
                packages.get(&metadata_key).is_some_and(|metadata| {
                    inferred_platform(
                        &metadata_key.name.to_string(),
                        WantedPlatformRef {
                            os: metadata.os.as_deref(),
                            cpu: metadata.cpu.as_deref(),
                            libc: metadata.libc.as_deref(),
                        },
                    )
                    .is_some()
                })
            }
        })
}

/// True if a single metadata row carries a constraint pacquet would
/// actually evaluate. Distinguishes "field present" from "field present
/// AND meaningful":
///
/// - `engines`: only `node` / `pnpm` keys matter. A package that
///   declares `engines.npm = ">=8"` (and nothing else) has no
///   constraint pacquet evaluates — pacquet isn't npm.
/// - `cpu` / `os` / `libc`: a `["any"]` value short-circuits to
///   "accept" inside `check_platform`'s `check_list`, and an empty
///   list cannot exclude the host either. Treat both as no-constraint.
fn metadata_has_meaningful_constraint(metadata: &PackageMetadata) -> bool {
    let engines_meaningful = metadata
        .engines
        .as_ref()
        .is_some_and(|engines| engines.contains_key("node") || engines.contains_key("pnpm"));
    engines_meaningful
        || platform_axis_meaningful(metadata.cpu.as_deref())
        || platform_axis_meaningful(metadata.os.as_deref())
        || platform_axis_meaningful(metadata.libc.as_deref())
}

/// One axis of `cpu` / `os` / `libc` carries no constraint when the
/// list is absent, empty, or exactly the `["any"]` sentinel that
/// `check_list` short-circuits as "accept everything".
fn platform_axis_meaningful(axis: Option<&[String]>) -> bool {
    match axis {
        None | Some([]) => false,
        Some([only]) if only == "any" => false,
        Some(_) => true,
    }
}

fn manifest_from_metadata(
    metadata_key: &PackageKey,
    metadata: &PackageMetadata,
) -> PackageInstallabilityManifest {
    PackageInstallabilityManifest {
        name: metadata_key.name.to_string(),
        engines: metadata.engines.as_ref().map(|map| WantedEngine {
            node: map.get("node").cloned(),
            pnpm: map.get("pnpm").cloned(),
        }),
        cpu: metadata.cpu.clone(),
        os: metadata.os.clone(),
        libc: metadata.libc.clone(),
    }
}

fn emit_skipped<Reporter: self::Reporter>(
    pkg_id: &str,
    reason: SkipReason,
    details: String,
    prefix: &str,
) {
    let (name, version) = split_name_version(pkg_id);
    let wire_reason = match reason {
        SkipReason::UnsupportedEngine => SkippedOptionalReason::UnsupportedEngine,
        SkipReason::UnsupportedPlatform => SkippedOptionalReason::UnsupportedPlatform,
    };
    Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some(details),
        package: SkippedOptionalPackage::Installed { id: pkg_id.to_string(), name, version },
        prefix: prefix.to_string(),
        reason: wire_reason,
    }));
}

/// Split a `name@version` (with possible leading `@` for scoped
/// packages) into `(name, version)`. Mirrors the `lastIndexOf('@')`
/// rule pacquet's manifest parser already uses.
fn split_name_version(pkg_id: &str) -> (String, String) {
    match pkg_id.rfind('@') {
        Some(idx) if idx > 0 => (pkg_id[..idx].to_string(), pkg_id[idx + 1..].to_string()),
        _ => (pkg_id.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests;
