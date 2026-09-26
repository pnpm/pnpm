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

pub(crate) use patched::snapshot_is_patched;
pub use platform::{
    InstallabilityHost, any_installability_constraint, any_optional_installability_constraint,
    check_installability, manifest_with_inferred_platform, platform_manifest_from_resolve_result,
};
pub use skipped_snapshots::SkippedSnapshots;

use std::collections::{HashMap, HashSet};

use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, PackageMetadata, PkgVerPeer, Prefix, ProjectSnapshot,
    SnapshotEntry,
};
use pnpm_package_is_installable::{InstallabilityError, InstallabilityOptions, SkipReason};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};

mod patched;
mod platform;
mod reachability;
mod skipped_snapshots;

use patched::without_published_engines;
use platform::manifest_from_metadata;
use reachability::{LockfileEdgeReach, walk_lockfile_edges};

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

        let defer_engines =
            self.base_options.engine_strict && snapshot_is_patched(snapshot_key, Some(snapshot));
        let warn = self.snapshot_warn(&metadata_key, metadata, skip_check_optional, defer_engines)?;
        // Whatever the seed recorded, this pass's verdict replaces it.
        self.skipped.remove_installability(snapshot_key);
        let Some(warn) = warn else { return Ok(()) };

        if !required {
            self.record_skip::<Reporter>(snapshot_key, &metadata_key, &warn);
            return Ok(());
        }
        self.report_incompatible_required(
            &metadata_key,
            metadata,
            warn,
            skip_check_optional,
            defer_engines,
        )
    }

    fn snapshot_warn(
        &mut self,
        metadata_key: &PackageKey,
        metadata: &PackageMetadata,
        optional: bool,
        defer_engines: bool,
    ) -> Result<Option<InstallabilityError>, Box<InstallabilityError>> {
        if defer_engines {
            return without_published_engines(metadata_key, metadata, optional, &self.base_options);
        }
        cached_check(&mut self.check_cache, metadata_key, metadata, optional, &self.base_options)
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
        defer_engines: bool,
    ) -> Result<(), Box<InstallabilityError>> {
        // The required dispatch drops the optional-only
        // platform-from-name inference, so its verdict needs the
        // non-optional check.
        let warn = if skip_check_optional && defer_engines {
            without_published_engines(metadata_key, metadata, false, &self.base_options)?
        } else if skip_check_optional {
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

/// The Node.js version the root project's `node` runtime dependency is
/// locked to: the Node.js pnpm installs for the project.
#[must_use]
pub fn find_root_runtime_node_version(
    importers: &HashMap<String, ProjectSnapshot>,
) -> Option<String> {
    root_runtime_node_ver_peer(importers)?.version_semver().map(ToString::to_string)
}

/// The snapshot of the root project's `node` runtime dependency, the one
/// [`find_root_runtime_node_version`] reads the version of. A dependency's
/// own `engines.runtime` pin adds another `node@runtime:` snapshot, which
/// this never returns.
#[must_use]
pub fn find_root_runtime_node_key<'a>(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
) -> Option<&'a PackageKey> {
    let ver_peer = root_runtime_node_ver_peer(importers)?;
    snapshots
        .keys()
        .find(|key| key.name.scope.is_none() && key.name.bare == "node" && key.suffix == *ver_peer)
}

pub(crate) fn root_runtime_node_ver_peer(
    importers: &HashMap<String, ProjectSnapshot>,
) -> Option<&PkgVerPeer> {
    importers
        .get(Lockfile::ROOT_IMPORTER_KEY)?
        .dependencies_by_groups([
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
        ])
        .filter(|(alias, _)| alias.scope.is_none() && alias.bare == "node")
        .filter_map(|(_, spec)| spec.version.ver_peer())
        .find(|ver_peer| ver_peer.prefix() == Prefix::Runtime)
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
