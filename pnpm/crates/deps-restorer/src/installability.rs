//! Per-install installability pass.
//!
//! For each snapshot in a frozen-lockfile install, run
//! `pnpm-package-is-installable`'s `check_package` against the
//! matching `PackageMetadata` and the host environment, build the
//! [`SkippedSnapshots`] set, and emit
//! `pnpm:skipped-optional-dependency` for every optional+incompatible
//! one.
//!
//! Pacquet's install path is lockfile-driven and has no resolver, so
//! the headless re-check is the only relevant emit site. Running it
//! every install also means the set is recomputed against the current
//! host, since the host arch may have changed since the previous
//! install wrote `.modules.yaml`.

pub use platform::{
    InstallabilityHost, any_installability_constraint, any_optional_installability_constraint,
    check_installability, manifest_with_inferred_platform, platform_manifest_from_resolve_result,
};

mod reachability;
use reachability::{LockfileEdgeReach, walk_lockfile_edges};

mod platform;
use platform::manifest_from_metadata;

use std::collections::{HashMap, HashSet};

use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, ProjectSnapshot, SnapshotEntry,
};
use pnpm_package_is_installable::{InstallabilityError, InstallabilityOptions, SkipReason};
use pnpm_reporter::{
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
///   subsequent install.
///
/// - **Fetch-failure skips** (`fetch_failed`) — an `optional: true`
///   snapshot whose tarball / metadata / extract step blew up
///   during the install. **Not** persisted: the catch site never
///   records the skip, so a subsequent install retries the fetch.
///
/// - **`--no-optional` exclusions** (`optional_excluded`) —
///   snapshots whose lockfile entry has `optional: true` AND the
///   user passed `--no-optional` (or `IncludedDependencies::optional_dependencies`
///   is false). **Not** persisted: the filter sits downstream of the
///   skip set, so re-running without
///   `--no-optional` brings the snapshots back into the install
///   graph. Pacquet's downstream architecture walks the lockfile
///   directly rather than a pre-pruned graph, so a separate filter
///   is needed here.
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
    #[must_use]
    pub fn from_set(set: HashSet<PackageKey>) -> Self {
        Self { installability: set, ..Self::default() }
    }

    /// Seed the installability set with snapshot keys recorded as
    /// skipped by a previous install (read from
    /// `.modules.yaml.skipped`). Unparsable strings are silently
    /// dropped — the seed is only consulted by membership lookup; a
    /// nonsense string never matches any current snapshot, so the
    /// orphan is harmless.
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

    pub fn add_fetch_failed_all(&mut self, keys: impl IntoIterator<Item = PackageKey>) {
        for key in keys {
            self.add_fetch_failed(key);
        }
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
    pub fn contains_optional_excluded(&self, key: &PackageKey) -> bool {
        self.optional_excluded.contains(key)
    }

    /// The same set with the `installability` subset dropped — the
    /// skips a written current lockfile has to reflect.
    ///
    /// `.modules.yaml.skipped` carries the installability subset from
    /// one install to the next, so the current lockfile can keep those
    /// entries and still describe what is on disk. It has to keep
    /// them: pnpm's current lockfile does, and a repeat install's
    /// "already up to date" comparison against the wanted lockfile
    /// would otherwise never match again once a platform-incompatible
    /// optional dependency was skipped. Fetch failures and
    /// `--no-optional` exclusions are recorded nowhere else, so
    /// leaving those out is what makes the next install redo them.
    #[must_use]
    pub fn transient_only(&self) -> Self {
        Self {
            installability: HashSet::new(),
            fetch_failed: self.fetch_failed.clone(),
            optional_excluded: self.optional_excluded.clone(),
        }
    }

    pub fn retain_installability_for_optional_snapshots(
        &mut self,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) {
        self.installability
            .retain(|key| snapshots.get(key).is_some_and(|snapshot| snapshot.optional));
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

    /// Insert into the installability set — the persisted subset
    /// written to `.modules.yaml.skipped`.
    pub fn insert_installability(&mut self, key: PackageKey) {
        self.installability.insert(key);
    }

    #[must_use]
    pub fn contains_installability(&self, key: &PackageKey) -> bool {
        self.installability.contains(key)
    }

    /// Drop a seeded installability skip whose package passes the
    /// current installability check — e.g. after `--os` / `--cpu` /
    /// `supportedArchitectures` changed between installs.
    pub fn remove_installability(&mut self, key: &PackageKey) {
        self.installability.remove(key);
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

/// Compute the [`SkippedSnapshots`] set for an install.
///
/// Installability is dispatched per inbound edge over the lockfile
/// graph, mirroring pnpm's resolve-time `packageIsInstallable`:
///
/// - An incompatible snapshot whose every inbound edge is an
///   `optionalDependencies` edge, or comes from a skipped parent, is
///   added to the set and reported via
///   `pnpm:skipped-optional-dependency`.
/// - An incompatible snapshot with at least one non-optional inbound
///   edge from an installed (non-skipped) importer or snapshot is the
///   install error when `engine_strict` is set, and otherwise emits
///   `tracing::warn!` and proceeds — even when the snapshot is also
///   optionally reachable. (The warn should emit `pnpm:install-check`,
///   which pacquet's reporter does not yet expose — slice 1
///   follow-up.)
/// - A snapshot the walk cannot reach from any importer applies the
///   same dispatch to its lockfile-propagated
///   [`SnapshotEntry::optional`] flag instead.
///
/// A *compatible* snapshot reachable only through skipped parents is
/// not added here; the dependency-closure extension that runs after
/// this pass records it.
///
/// Snapshots without a matching `PackageMetadata` row are skipped
/// over — `CreateVirtualStore` errors on them separately. An invalid
/// `nodeVersion` surfaces as `ERR_PNPM_INVALID_NODE_VERSION`
/// regardless of edges and strictness.
pub fn compute_skipped_snapshots<Reporter: self::Reporter>(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    host: &InstallabilityHost,
    prefix: &str,
    mut seed: SkippedSnapshots,
) -> Result<SkippedSnapshots, Box<InstallabilityError>> {
    seed.retain_installability_for_optional_snapshots(snapshots);

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
    // The filtered `seed` is returned on the fast path so previously
    // skipped optional snapshots survive across reinstalls even when
    // the lockfile's per-snapshot constraints have since been removed.
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

    // Build the host-derived part of the options once. Only the
    // (`engine_strict`-irrelevant) `optional` flag varies per
    // dispatch, but the result of [`check_package`] — "does this
    // manifest satisfy the host?" — does not. The check verdict is
    // cached per peer-stripped `metadata_key` (see [`cached_check`]);
    // the per-snapshot loop then only needs to apply the
    // optional / engine-strict dispatch.
    let base_options = InstallabilityOptions {
        engine_strict: host.engine_strict,
        // Cache-shared check: `optional` is applied per dispatch
        // below, not inside `check_package`.
        optional: false,
        current_node_version: host.node_version.as_str(),
        pnpm_version: None,
        current_os: host.os,
        current_cpu: host.cpu,
        current_libc: host.libc,
        supported_architectures: host.supported_architectures.as_ref(),
    };

    let mut check_cache = CheckCache::new();
    let reach =
        walk_lockfile_edges(importers, snapshots, packages, &base_options, &mut check_cache)?;
    let mut scan = SkipScan {
        packages,
        host,
        prefix,
        base_options,
        check_cache,
        reach,
        seen_emit: HashSet::new(),
        skipped: seed,
    };

    for (snapshot_key, snapshot) in snapshots {
        scan.classify::<Reporter>(snapshot_key, snapshot)?;
    }

    Ok(scan.skipped)
}

/// The per-snapshot dispatch of [`compute_skipped_snapshots`] and the
/// state it threads through the lockfile's snapshots.
struct SkipScan<'a, 'lock> {
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    host: &'a InstallabilityHost,
    prefix: &'a str,
    base_options: InstallabilityOptions<'a>,
    check_cache: CheckCache,
    reach: LockfileEdgeReach<'lock>,
    /// Metadata keys already reported, so peer variants of one package
    /// emit a single skip log.
    seen_emit: HashSet<PackageKey>,
    skipped: SkippedSnapshots,
}

impl SkipScan<'_, '_> {
    fn classify<Reporter: self::Reporter>(
        &mut self,
        snapshot_key: &PackageKey,
        snapshot: &SnapshotEntry,
    ) -> Result<(), Box<InstallabilityError>> {
        // A seeded installability skip is re-evaluated here — the host,
        // `supportedArchitectures`, or the package's constraints may
        // have changed since the skip was recorded, and pnpm recomputes
        // installability fresh on every install. The other categories
        // carry per-run state, not a verdict to re-check.
        let seeded = self.skipped.contains_installability(snapshot_key);
        if !seeded && self.skipped.contains(snapshot_key) {
            return Ok(());
        }

        let metadata_key = snapshot_key.without_peer();
        let Some(metadata) = self.packages.get(&metadata_key) else { return Ok(()) };

        // Reachable snapshots dispatch on their inbound edges; the rest
        // keep the lockfile-propagated flag. The skip check runs with
        // `optional: true` whenever a skip is possible so the
        // platform-from-name inference applies to it.
        let (skip_check_optional, required) = if self.reach.reachable.contains(snapshot_key) {
            (true, self.reach.required.contains(snapshot_key))
        } else {
            (snapshot.optional, !snapshot.optional)
        };

        let warn = cached_check(
            &mut self.check_cache,
            &metadata_key,
            metadata,
            skip_check_optional,
            &self.base_options,
        )?;
        // Whatever the seed recorded, this pass's verdict replaces it.
        self.skipped.remove_installability(snapshot_key);
        let Some(warn) = warn else { return Ok(()) };

        if !required {
            self.record_skip::<Reporter>(snapshot_key, &metadata_key, &warn);
            return Ok(());
        }
        self.report_incompatible_required(&metadata_key, metadata, warn, skip_check_optional)
    }

    fn record_skip<Reporter: self::Reporter>(
        &mut self,
        snapshot_key: &PackageKey,
        metadata_key: &PackageKey,
        warn: &InstallabilityError,
    ) {
        self.skipped.insert_installability(snapshot_key.clone());
        if self.seen_emit.insert(metadata_key.clone()) {
            emit_skipped::<Reporter>(
                &metadata_key.to_string(),
                warn.skip_reason(),
                warn.to_string(),
                self.prefix,
            );
        }
    }

    /// A package that an installed non-optional edge reaches cannot be
    /// skipped: under `engine-strict` it fails the install, otherwise
    /// it warns.
    fn report_incompatible_required(
        &mut self,
        metadata_key: &PackageKey,
        metadata: &PackageMetadata,
        warn: InstallabilityError,
        skip_check_optional: bool,
    ) -> Result<(), Box<InstallabilityError>> {
        // The required dispatch drops the optional-only
        // platform-from-name inference, so its verdict needs the
        // non-optional check.
        let warn = if skip_check_optional {
            cached_check(&mut self.check_cache, metadata_key, metadata, false, &self.base_options)?
        } else {
            Some(warn)
        };
        let Some(warn) = warn else { return Ok(()) };

        if self.host.engine_strict {
            return Err(Box::new(warn));
        }

        // Required, non-strict: this should emit a
        // `pnpm:install-check` warn (TODO: add channel to the
        // reporter). For now the tracing-level warning is the
        // user-visible signal that an incompatible required dep slipped
        // through.
        tracing::warn!(
            target: "pacquet::install",
            package = %metadata_key,
            "{}",
            warn,
        );
        Ok(())
    }
}

/// `--no-runtime` (or `config.skip_runtimes`): add every project-direct
/// runtime dependency (a `@runtime:` snapshot key with a binary
/// resolution) to the skip set, keeping its archive unfetched and its
/// bins unlinked while the resolved entry stays in the lockfile.
/// Shared by the frozen- and fresh-lockfile install paths, which run it
/// right before the dependency-closure extension.
///
/// The skips reuse the transient bucket of
/// [`SkippedSnapshots::add_optional_excluded`], so — like
/// `--no-optional` — the exclusion is never persisted into
/// `.modules.yaml.skipped`.
pub fn add_direct_runtime_skips(
    skipped: &mut SkippedSnapshots,
    importers: &HashMap<String, ProjectSnapshot>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) {
    for importer in importers.values() {
        for dep_map in [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            add_runtime_skips_from(dep_map, packages, skipped);
        }
    }
}

fn add_runtime_skips_from(
    dep_map: &pnpm_lockfile::ResolvedDependencyMap,
    packages: &HashMap<PackageKey, PackageMetadata>,
    skipped: &mut SkippedSnapshots,
) {
    for (alias, spec) in dep_map {
        // Build the candidate snapshot key. For non-aliased deps this
        // is `(alias, version)`; for aliased deps it's the alias's own
        // (name, suffix). `link:` deps are skipped.
        let Some(key) = spec.version.resolved_key(alias) else { continue };
        if !key.to_string().contains("@runtime:") {
            continue;
        }
        if let Some(meta) = packages.get(&key)
            && matches!(
                &meta.resolution,
                LockfileResolution::Binary(_) | LockfileResolution::Variations(_),
            )
        {
            skipped.add_optional_excluded(key);
        }
    }
}

/// `None` = compatible. `Some(err)` = incompatible, with the
/// diagnostic the caller would surface (the skip's `details` payload
/// or the warn / engine-strict error).
///
/// The key carries the `optional` flag alongside the peer-stripped
/// metadata key because the platform-from-name inference only runs
/// for optional dispatches, so the two verdicts of one metadata row
/// can differ. Sharing entries across peer-variants pays off on
/// lockfiles with peer-resolved variants of the same package
/// (`react-dom@17(react@17)` / `react-dom@17(react@18)`, etc.);
/// `InstallabilityOptions` borrows its string fields for exactly this
/// reuse pattern.
type CheckCache = HashMap<(PackageKey, bool), Option<InstallabilityError>>;

fn cached_check(
    check_cache: &mut CheckCache,
    metadata_key: &PackageKey,
    metadata: &PackageMetadata,
    optional: bool,
    base_options: &InstallabilityOptions<'_>,
) -> Result<Option<InstallabilityError>, Box<InstallabilityError>> {
    let cache_key = (metadata_key.clone(), optional);
    if let Some(cached) = check_cache.get(&cache_key) {
        return Ok(cached.clone());
    }
    let manifest = manifest_from_metadata(metadata_key, metadata);
    let pkg_id = metadata_key.to_string();
    let options = InstallabilityOptions { optional, ..*base_options };
    let verdict = check_installability(&pkg_id, &manifest, &options)?;
    check_cache.insert(cache_key, verdict.clone());
    Ok(verdict)
}

/// Checks lockfile package metadata as an optional dependency on the current host.
///
/// Returns `Ok(true)` when the package is compatible, `Ok(false)` for an
/// unsupported engine or platform, and propagates an invalid configured Node.js
/// version as [`InstallabilityError::InvalidNodeVersion`].
pub fn package_metadata_is_installable(
    metadata_key: &PackageKey,
    metadata: &PackageMetadata,
    host: &InstallabilityHost,
) -> Result<bool, Box<InstallabilityError>> {
    let manifest = manifest_from_metadata(metadata_key, metadata);
    let options = InstallabilityOptions {
        engine_strict: host.engine_strict,
        optional: true,
        current_node_version: host.node_version.as_str(),
        pnpm_version: None,
        current_os: host.os,
        current_cpu: host.cpu,
        current_libc: host.libc,
        supported_architectures: host.supported_architectures.as_ref(),
    };
    Ok(check_installability(&metadata_key.to_string(), &manifest, &options)?.is_none())
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
        parents: None,
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
