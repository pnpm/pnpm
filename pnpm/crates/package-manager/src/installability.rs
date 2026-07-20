//! Per-install installability pass.
//!
//! For each snapshot in a frozen-lockfile install, run
//! `pacquet-package-is-installable`'s `check_package` against the
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

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet, VecDeque},
};

use pacquet_lockfile::{PackageKey, PackageMetadata, ProjectSnapshot, SnapshotEntry};
use pacquet_package_is_installable::{
    InstallabilityError, InstallabilityOptions, PackageInstallabilityManifest, SkipReason,
    SupportedArchitectures, WantedEngine, WantedPlatformRef, check_package, inferred_platform,
};
use pacquet_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};
use pacquet_resolving_resolver_base::ResolveResult;
use serde_json::Value;

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
    #[cfg(test)]
    pub(crate) fn from_set(set: HashSet<PackageKey>) -> Self {
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
    pub(crate) fn contains_optional_excluded(&self, key: &PackageKey) -> bool {
        self.optional_excluded.contains(key)
    }

    pub(crate) fn retain_installability_for_optional_snapshots(
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
    pub(crate) fn insert_installability(&mut self, key: PackageKey) {
        self.installability.insert(key);
    }

    #[must_use]
    pub(crate) fn contains_installability(&self, key: &PackageKey) -> bool {
        self.installability.contains(key)
    }

    /// Drop a seeded installability skip whose package passes the
    /// current installability check — e.g. after `--os` / `--cpu` /
    /// `supportedArchitectures` changed between installs.
    pub(crate) fn remove_installability(&mut self, key: &PackageKey) {
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
    /// synthetic. [`Self::detect_with`] overrides both the version
    /// (the `nodeVersion` setting) and the engine-strict policy.
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

    /// Build the host context with a caller-supplied engine-strict policy and
    /// optional Node.js version override (the `engineStrict` / `nodeVersion`
    /// config settings).
    ///
    /// An explicit `node_version` is authoritative: no `node --version` probe
    /// runs and `node_detected` is `true`, so the side-effects cache keys off
    /// the pinned major exactly as it would off a detected one. A leading `v`
    /// (as in `process.version` / `node --version`, e.g. `v22.11.0`) is
    /// stripped so the value parses as exact semver, matching the auto-detect
    /// path. `None` falls back to [`Self::detect`], then overrides
    /// `engine_strict`.
    #[must_use]
    pub fn detect_with(engine_strict: bool, node_version: Option<String>) -> Self {
        match node_version.map(|version| normalize_node_version(&version)) {
            Some(node_version) => Self {
                node_version,
                node_detected: true,
                os: pacquet_graph_hasher::host_platform(),
                cpu: pacquet_graph_hasher::host_arch(),
                libc: pacquet_graph_hasher::host_libc(),
                supported_architectures: None,
                engine_strict,
            },
            None => Self { engine_strict, ..Self::detect() },
        }
    }
}

/// Canonicalize a Node.js version string for the engine check: trim surrounding
/// whitespace and drop a single leading `v` (`v22.11.0` → `22.11.0`) so a value
/// copied from `process.version` / `node --version` parses as exact semver.
fn normalize_node_version(version: &str) -> String {
    let trimmed = version.trim();
    trimmed.strip_prefix('v').unwrap_or(trimmed).to_string()
}

pub(crate) fn check_installability(
    package_id: &str,
    manifest: &PackageInstallabilityManifest,
    options: &InstallabilityOptions<'_>,
) -> Result<Option<InstallabilityError>, Box<InstallabilityError>> {
    let manifest = if options.optional {
        manifest_with_inferred_platform(manifest)
    } else {
        Cow::Borrowed(manifest)
    };
    check_package(package_id, manifest.as_ref(), options)
        .map_err(|invalid| Box::new(InstallabilityError::InvalidNodeVersion(invalid)))
}

pub(crate) fn manifest_with_inferred_platform(
    manifest: &PackageInstallabilityManifest,
) -> Cow<'_, PackageInstallabilityManifest> {
    let Some(platform) = inferred_platform(
        &manifest.name,
        WantedPlatformRef {
            os: manifest.os.as_deref(),
            cpu: manifest.cpu.as_deref(),
            libc: manifest.libc.as_deref(),
        },
    ) else {
        return Cow::Borrowed(manifest);
    };
    Cow::Owned(PackageInstallabilityManifest {
        name: manifest.name.clone(),
        engines: manifest.engines.clone(),
        os: platform.os,
        cpu: platform.cpu,
        libc: platform.libc,
    })
}

pub(crate) fn platform_manifest_from_resolve_result(
    result: &ResolveResult,
    fallback_alias: Option<&str>,
) -> PackageInstallabilityManifest {
    let manifest = result.manifest.as_deref();
    PackageInstallabilityManifest {
        name: result
            .name_ver
            .as_ref()
            .map(|name_ver| name_ver.name.to_string())
            .or_else(|| {
                manifest
                    .and_then(|manifest| manifest.get("name"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| result.alias.clone())
            .or_else(|| fallback_alias.map(ToString::to_string))
            .unwrap_or_default(),
        engines: None,
        cpu: read_string_list(manifest, "cpu"),
        os: read_string_list(manifest, "os"),
        libc: read_string_list(manifest, "libc"),
    }
}

fn read_string_list(manifest: Option<&Value>, key: &str) -> Option<Vec<String>> {
    let value = manifest?.get(key)?;
    let out: Vec<String> = match value {
        Value::String(value) => vec![value.clone()],
        Value::Array(items) => {
            items.iter().filter_map(Value::as_str).map(ToString::to_string).collect()
        }
        _ => Vec::new(),
    };
    (!out.is_empty()).then_some(out)
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

    let mut skipped = seed;
    let mut seen_emit: HashSet<PackageKey> = HashSet::new();

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

    for (snapshot_key, snapshot) in snapshots {
        // A seeded installability skip is re-evaluated below — the
        // host, `supportedArchitectures`, or the package's constraints
        // may have changed since the skip was recorded, and pnpm
        // recomputes installability fresh on every install. The other
        // categories carry per-run state, not a verdict to re-check.
        let seeded = skipped.contains_installability(snapshot_key);
        if !seeded && skipped.contains(snapshot_key) {
            continue;
        }

        let metadata_key = snapshot_key.without_peer();
        let Some(metadata) = packages.get(&metadata_key) else { continue };

        // Reachable snapshots dispatch on their inbound edges; the
        // rest keep the lockfile-propagated flag. The skip check runs
        // with `optional: true` whenever a skip is possible so the
        // platform-from-name inference applies to it.
        let (skip_check_optional, required) = if reach.reachable.contains(snapshot_key) {
            (true, reach.required.contains(snapshot_key))
        } else {
            (snapshot.optional, !snapshot.optional)
        };

        let warn = cached_check(
            &mut check_cache,
            &metadata_key,
            metadata,
            skip_check_optional,
            &base_options,
        )?;
        let Some(warn) = warn else {
            if seeded {
                skipped.remove_installability(snapshot_key);
            }
            continue;
        };

        if !required {
            skipped.insert_installability(snapshot_key.clone());
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

        // The required (non-optional-edge) dispatch drops the
        // optional-only platform-from-name inference, so its verdict
        // needs the non-optional check.
        let warn = if skip_check_optional {
            cached_check(&mut check_cache, &metadata_key, metadata, false, &base_options)?
        } else {
            Some(warn)
        };
        if seeded {
            skipped.remove_installability(snapshot_key);
        }
        let Some(warn) = warn else { continue };

        if host.engine_strict {
            return Err(Box::new(warn));
        }

        // Required, non-strict: this should emit a
        // `pnpm:install-check` warn (TODO: add channel to the reporter).
        // For now the tracing-level warning is the user-visible signal
        // that an incompatible required dep slipped through.
        tracing::warn!(
            target: "pacquet::install",
            package = %metadata_key,
            "{}",
            warn,
        );
    }

    Ok(skipped)
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

/// Edge classification produced by [`walk_lockfile_edges`].
struct LockfileEdgeReach<'lock> {
    /// Snapshots reachable from any importer through any edge chain,
    /// including chains through skipped parents.
    reachable: HashSet<&'lock PackageKey>,
    /// Skip candidates (incompatible when checked as optional) with a
    /// non-optional inbound edge from an installed source — an
    /// importer or a non-skipped snapshot. The fail / warn dispatch
    /// wins for these.
    required: HashSet<&'lock PackageKey>,
}

/// Walk the lockfile graph from every importer and classify each
/// snapshot's inbound edges for the per-edge dispatch in
/// [`compute_skipped_snapshots`].
fn walk_lockfile_edges<'lock>(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &'lock HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    base_options: &InstallabilityOptions<'_>,
    check_cache: &mut CheckCache,
) -> Result<LockfileEdgeReach<'lock>, Box<InstallabilityError>> {
    let mut reachable: HashSet<&'lock PackageKey> = HashSet::new();
    let mut queue: VecDeque<&'lock PackageKey> = VecDeque::new();
    for importer in importers.values() {
        for (target, _) in importer_edges(importer) {
            if let Some((key, _)) = snapshots.get_key_value(&target)
                && reachable.insert(key)
            {
                queue.push_back(key);
            }
        }
    }
    while let Some(key) = queue.pop_front() {
        for (target, _) in snapshot_edges(&snapshots[key]) {
            if let Some((child, _)) = snapshots.get_key_value(&target)
                && reachable.insert(child)
            {
                queue.push_back(child);
            }
        }
    }

    // Propagate installed-ness down from the importers. A skip
    // candidate is installed only once a non-optional edge from an
    // installed source reaches it; every other snapshot is installed
    // as soon as any edge from an installed source does. Edges out of
    // a never-installed candidate are not expanded, so a subtree
    // behind a skipped parent stays skippable no matter what edge
    // kinds it uses internally.
    let mut installed: HashSet<&'lock PackageKey> = HashSet::new();
    let mut required: HashSet<&'lock PackageKey> = HashSet::new();
    let mut pending: VecDeque<(&'lock PackageKey, bool)> = importers
        .values()
        .flat_map(importer_edges)
        .filter_map(|(target, edge_optional)| {
            snapshots.get_key_value(&target).map(|(key, _)| (key, edge_optional))
        })
        .collect();
    while let Some((key, edge_optional)) = pending.pop_front() {
        let metadata_key = key.without_peer();
        let skip_candidate = match packages.get(&metadata_key) {
            Some(metadata) => {
                cached_check(check_cache, &metadata_key, metadata, true, base_options)?.is_some()
            }
            None => false,
        };
        let newly_installed = if skip_candidate {
            if edge_optional {
                false
            } else {
                required.insert(key);
                installed.insert(key)
            }
        } else {
            installed.insert(key)
        };
        if newly_installed {
            for (target, child_edge_optional) in snapshot_edges(&snapshots[key]) {
                if let Some((child, _)) = snapshots.get_key_value(&target) {
                    pending.push_back((child, child_edge_optional));
                }
            }
        }
    }

    Ok(LockfileEdgeReach { reachable, required })
}

/// Iterate an importer's resolvable direct-dep edges as
/// `(snapshot key, edge is optional)` pairs. `dependencies` and
/// `devDependencies` are non-optional edges; `link:` entries resolve
/// to sibling importers, which are walk roots already, and are
/// dropped.
fn importer_edges(importer: &ProjectSnapshot) -> impl Iterator<Item = (PackageKey, bool)> + '_ {
    let required = importer
        .dependencies
        .iter()
        .chain(importer.dev_dependencies.iter())
        .flatten()
        .filter_map(|(name, spec)| spec.version.resolved_key(name))
        .map(|key| (key, false));
    let optional = importer
        .optional_dependencies
        .iter()
        .flatten()
        .filter_map(|(name, spec)| spec.version.resolved_key(name))
        .map(|key| (key, true));
    required.chain(optional)
}

/// Iterate a snapshot's resolvable dep edges as
/// `(snapshot key, edge is optional)` pairs.
fn snapshot_edges(snapshot: &SnapshotEntry) -> impl Iterator<Item = (PackageKey, bool)> + '_ {
    let required = snapshot
        .dependencies
        .iter()
        .flatten()
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
        .map(|key| (key, false));
    let optional = snapshot
        .optional_dependencies
        .iter()
        .flatten()
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
        .map(|key| (key, true));
    required.chain(optional)
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
                        metadata_key.name.bare.as_str(),
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

#[must_use]
pub fn any_optional_installability_constraint(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> bool {
    snapshots.iter().any(|(snapshot_key, snapshot)| {
        if !snapshot.optional {
            return false;
        }
        let metadata_key = snapshot_key.without_peer();
        packages.get(&metadata_key).is_some_and(|metadata| {
            metadata_has_meaningful_constraint(metadata)
                || inferred_platform(
                    metadata_key.name.bare.as_str(),
                    WantedPlatformRef {
                        os: metadata.os.as_deref(),
                        cpu: metadata.cpu.as_deref(),
                        libc: metadata.libc.as_deref(),
                    },
                )
                .is_some()
        })
    })
}

/// True if a single metadata row carries a constraint pacquet would
/// actually evaluate.
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
