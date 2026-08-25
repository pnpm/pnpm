//! Verify a `pnpm-lock.yaml` is still up-to-date with the project's
//! `package.json` before a `--frozen-lockfile` install proceeds.
//!
//! Pacquet's frozen-lockfile path materializes `node_modules` from
//! whatever the lockfile says, on the assumption that the lockfile is
//! the contract between the user's manifest and the install. If the
//! manifest has drifted (deps added/removed/bumped without re-running
//! the resolver), pacquet installs the wrong shape of `node_modules`
//! and the drift goes undiagnosed.
//!
//! This module runs a per-importer structural comparison that returns
//! the first mismatch (if any) as a typed [`StalenessReason`]. The
//! frozen-lockfile dispatcher surfaces this as
//! `ERR_PNPM_OUTDATED_LOCKFILE`, which is the CI-correctness contract.

use crate::{Lockfile, ProjectSnapshot};
use derive_more::{Display, Error};
use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_parse_wanted_dependency::git_specifiers_are_equivalent;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Copy)]
pub struct LockfileSettingsCheck<'a> {
    pub catalogs: &'a Catalogs,
    pub overrides: Option<&'a HashMap<String, String>>,
    pub package_extensions_checksum: Option<&'a str>,
    pub ignored_optional_dependencies: Option<&'a [String]>,
    pub patched_dependencies: Option<&'a BTreeMap<String, String>>,
    pub auto_install_peers: bool,
    pub dedupe_peers: bool,
    pub exclude_links_from_lockfile: bool,
    pub inject_workspace_packages: bool,
    pub peers_suffix_max_length: u64,
    pub pnpmfile_checksum: PnpmfileChecksumCheck<'a>,
}

/// What [`check_lockfile_settings`] compares the lockfile's
/// `pnpmfileChecksum` against.
///
/// The value is not a plain config read: pnpm records a checksum only
/// when a loaded pnpmfile exports a `hooks` object, so computing it can
/// mean evaluating the pnpmfile. Callers that already know the
/// pnpmfiles are unchanged — or that have no pnpmfile to speak of —
/// say so with [`PnpmfileChecksumCheck::Skip`] instead of inventing a
/// value, since passing `Current(None)` would fail every install in a
/// project whose lockfile legitimately records one.
#[derive(Clone, Copy)]
pub enum PnpmfileChecksumCheck<'a> {
    /// The checksum the current install would record: the hash of the
    /// pnpmfiles that export hooks, `None` when none do.
    Current(Option<&'a str>),

    /// Leave `pnpmfileChecksum` uncompared.
    Skip,
}

/// Why an importer's lockfile entry doesn't satisfy the on-disk
/// `package.json`. A typed enum so callers can match on the
/// discriminant without parsing format strings, and tests can assert
/// against the shape rather than the wording.
#[derive(Debug, Display, Error, PartialEq)]
#[non_exhaustive]
pub enum StalenessReason {
    /// A catalog entry recorded in the lockfile's `catalogs:` snapshot
    /// no longer matches the current workspace catalog config. This is
    /// the first drift branch checked, surfaced when
    /// `all_catalogs_are_up_to_date` fails.
    #[display("`catalogs` in the lockfile don't match the current config")]
    CatalogsChanged { lockfile: Option<crate::CatalogSnapshots>, config: Catalogs },

    /// The lockfile has no `importers["."]` (or whatever id) entry,
    /// so we can't even start the comparison.
    #[display(r#"the lockfile has no `importers["{importer_id}"]` entry"#)]
    NoImporter { importer_id: String },

    /// The lockfile records an importer for a workspace project that
    /// no longer exists. Only reported for an unfiltered install of the
    /// whole workspace, where the project list is the complete one.
    #[display(r#"the lockfile records `importers["{importer_id}"]`, but no such project exists"#)]
    RemovedImporter { importer_id: String },

    /// The flat union of `dependencies ∪ devDependencies ∪
    /// optionalDependencies` from the manifest doesn't match the
    /// per-dep specifiers recorded in the importer entry: the
    /// specifiers in the lockfile don't match the specifiers in
    /// package.json.
    #[display("specifiers in the lockfile don't match specifiers in package.json:{_0}")]
    SpecifiersDiffer(#[error(not(source))] SpecDiff),

    /// `publishDirectory` on the importer entry doesn't match
    /// `publishConfig.directory` on the manifest.
    #[display(
        "`publishDirectory` in the lockfile ({lockfile:?}) doesn't match `publishConfig.directory` in package.json ({manifest:?})"
    )]
    PublishDirectoryMismatch { lockfile: Option<String>, manifest: Option<String> },

    /// `dependenciesMeta` on the importer doesn't match
    /// `dependenciesMeta` on the manifest.
    #[display(
        "importer dependencies meta ({lockfile}) doesn't match package manifest dependencies meta ({manifest})"
    )]
    DependenciesMetaMismatch { lockfile: String, manifest: String },

    /// The recorded specifier for one dep diverges from the manifest's
    /// specifier for the same dep.
    #[display(
        "importer {field}.{name} specifier {lockfile:?} doesn't match package manifest specifier ({manifest:?})"
    )]
    DepSpecifierMismatch { field: &'static str, name: String, lockfile: String, manifest: String },

    /// A semver resolution recorded for a direct dependency no longer
    /// satisfies its unchanged manifest range. This catches a broken
    /// lockfile whose specifier map still agrees with `package.json`.
    #[display(
        "the importer resolution is broken at dependency {name:?}: version {version:?} doesn't satisfy range {range:?}"
    )]
    ResolutionDoesNotSatisfy { name: String, version: String, range: String },

    /// The lockfile's `ignoredOptionalDependencies` (sorted) differs
    /// from the current install's `Config::ignored_optional_dependencies`
    /// (sorted). This drift would otherwise require a full resolution;
    /// pacquet has no resolver, so the matching action is to surface
    /// this as `OutdatedLockfile`. Both values are returned sorted so
    /// the error message reads stably in CI logs.
    #[display(
        "`ignoredOptionalDependencies` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    IgnoredOptionalDependenciesChanged { lockfile: Vec<String>, config: Vec<String> },

    /// The lockfile's `overrides` map doesn't match the current
    /// install's `Config::overrides`. This drift would otherwise
    /// require a full resolution; pacquet has no resolver, so the
    /// matching action is to surface this as `OutdatedLockfile`. Both
    /// values are normalized into a `BTreeMap` so the comparison is
    /// order-insensitive (an absent map equals an empty one), and the
    /// rendered error reads stably.
    #[display(
        "`overrides` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    OverridesChanged { lockfile: BTreeMap<String, String>, config: BTreeMap<String, String> },

    /// The lockfile's `settings.injectWorkspacePackages` differs from
    /// the current install's `Config::inject_workspace_packages`. The
    /// gate normalizes both sides to a boolean so an absent setting
    /// equals an explicit `false`. This drift would otherwise require a
    /// full resolution; pacquet has no resolver, so the matching action
    /// is to surface this as `OutdatedLockfile`.
    #[display(
        "`injectWorkspacePackages` in the lockfile ({lockfile}) doesn't match the current config ({config})"
    )]
    InjectWorkspacePackagesChanged { lockfile: bool, config: bool },

    /// `settings.peersSuffixMaxLength` in the lockfile differs from
    /// the value the current install would use. An unset field in the
    /// lockfile is treated as the default (1000), so drift is "recorded
    /// value (or default) doesn't equal the current config's value".
    #[display(
        "`peersSuffixMaxLength` in the lockfile ({lockfile}) doesn't match the current config ({config})"
    )]
    PeersSuffixMaxLengthChanged { lockfile: u64, config: u64 },

    /// The lockfile's `packageExtensionsChecksum` doesn't match the
    /// checksum derived from the current install's
    /// `Config::package_extensions`. This drift would otherwise require
    /// a full resolution; pacquet has no resolver, so the matching
    /// action is to surface this as `OutdatedLockfile`. Both values are
    /// the prefixed `sha256-…` strings the writer emits.
    #[display(
        "`packageExtensionsChecksum` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    PackageExtensionsChecksumChanged { lockfile: Option<String>, config: Option<String> },

    /// The lockfile's `patchedDependencies` (key → patch-file hash)
    /// doesn't match the map the current install would write. This drift
    /// would otherwise require a full resolution; pacquet has no resolver,
    /// so the matching action is to surface this as `OutdatedLockfile`. A
    /// changed patch file changes its hash here, which is what catches an
    /// edited patch whose `(patch_hash=...)` depPath suffix would otherwise
    /// go stale. Both values are normalized into a `BTreeMap` so the
    /// comparison is order-insensitive.
    #[display(
        "`patchedDependencies` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    PatchedDependenciesChanged {
        lockfile: BTreeMap<String, String>,
        config: BTreeMap<String, String>,
    },

    /// The lockfile's `settings.autoInstallPeers` differs from the
    /// current install's `Config::auto_install_peers`. Only checked when
    /// the lockfile records a `settings` block, which is the only place
    /// the value it was written under is preserved.
    #[display(
        "`autoInstallPeers` in the lockfile ({lockfile}) doesn't match the current config ({config})"
    )]
    AutoInstallPeersChanged { lockfile: bool, config: bool },

    /// The lockfile's `settings.dedupePeers` differs from the current
    /// install's `Config::dedupe_peers`. The key is written only while
    /// the setting is on, so both sides normalize to a boolean and an
    /// absent key equals `false`.
    #[display(
        "`dedupePeers` in the lockfile ({lockfile}) doesn't match the current config ({config})"
    )]
    DedupePeersChanged { lockfile: bool, config: bool },

    /// The lockfile's `settings.excludeLinksFromLockfile` differs from
    /// the current install's `Config::exclude_links_from_lockfile`. The
    /// setting decides whether `link:` deps are recorded at all, so a
    /// flip invalidates every importer snapshot. Like
    /// [`Self::AutoInstallPeersChanged`], only checked when the lockfile
    /// records a `settings` block.
    #[display(
        "`excludeLinksFromLockfile` in the lockfile ({lockfile}) doesn't match the current config ({config})"
    )]
    ExcludeLinksFromLockfileChanged { lockfile: bool, config: bool },

    /// The lockfile's `pnpmfileChecksum` doesn't match the checksum the
    /// current install would record: a pnpmfile was added, edited, or
    /// removed since the lockfile was written, so the manifests the
    /// recorded resolution was built from are no longer the ones this
    /// install would see. Both values are the prefixed `sha256-…`
    /// strings, absent when no pnpmfile exports hooks.
    #[display(
        "`pnpmfileChecksum` in the lockfile ({lockfile:?}) doesn't match the current pnpmfile ({config:?})"
    )]
    PnpmfileChecksumChanged { lockfile: Option<String>, config: Option<String> },
}

impl StalenessReason {
    /// The name of the drifted setting, or `None` when the drift is
    /// between the lockfile and `package.json` rather than between the
    /// lockfile and the configuration.
    ///
    /// pnpm reports the two classes differently: settings drift is an
    /// `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` naming the field, manifest
    /// drift an `ERR_PNPM_OUTDATED_LOCKFILE` spelling out the diff. The
    /// names returned here are the ones pnpm's
    /// `getOutdatedLockfileSetting` reports, so an error message quotes
    /// the same field either stack produced it.
    #[must_use]
    pub fn setting_name(&self) -> Option<&'static str> {
        match self {
            StalenessReason::CatalogsChanged { .. } => Some("catalogs"),
            StalenessReason::OverridesChanged { .. } => Some("overrides"),
            StalenessReason::PackageExtensionsChecksumChanged { .. } => {
                Some("packageExtensionsChecksum")
            }
            StalenessReason::IgnoredOptionalDependenciesChanged { .. } => {
                Some("ignoredOptionalDependencies")
            }
            StalenessReason::PatchedDependenciesChanged { .. } => Some("patchedDependencies"),
            StalenessReason::AutoInstallPeersChanged { .. } => Some("settings.autoInstallPeers"),
            StalenessReason::DedupePeersChanged { .. } => Some("settings.dedupePeers"),
            StalenessReason::ExcludeLinksFromLockfileChanged { .. } => {
                Some("settings.excludeLinksFromLockfile")
            }
            StalenessReason::PeersSuffixMaxLengthChanged { .. } => {
                Some("settings.peersSuffixMaxLength")
            }
            StalenessReason::PnpmfileChecksumChanged { .. } => Some("pnpmfileChecksum"),
            StalenessReason::InjectWorkspacePackagesChanged { .. } => {
                Some("settings.injectWorkspacePackages")
            }
            StalenessReason::NoImporter { .. }
            | StalenessReason::RemovedImporter { .. }
            | StalenessReason::SpecifiersDiffer(_)
            | StalenessReason::PublishDirectoryMismatch { .. }
            | StalenessReason::DependenciesMetaMismatch { .. }
            | StalenessReason::DepSpecifierMismatch { .. }
            | StalenessReason::ResolutionDoesNotSatisfy { .. } => None,
        }
    }
}

/// Per-bucket diff against the manifest's flat union of deps.
/// Identical entries are omitted. Empty buckets render as nothing in
/// the `Display` impl so the resulting message lists only what the
/// user needs to fix.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SpecDiff {
    pub added: BTreeMap<String, String>,
    pub removed: BTreeMap<String, String>,
    pub modified: BTreeMap<String, (String, String)>,
    /// The lockfile importer the diff belongs to, when the caller
    /// checked a specific importer. Rendered into the message so a
    /// workspace-wide freshness failure names the project whose
    /// manifest drifted instead of only the dependency.
    pub importer_id: Option<String>,
}

impl std::fmt::Display for SpecDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(importer_id) = &self.importer_id {
            write!(f, "\n* in importers[{importer_id:?}]:")?;
        }
        // Singular/plural matters here: the diff is rendered into
        // `ERR_PNPM_OUTDATED_LOCKFILE` CI output, which users see
        // and may quote in issues. "1 dependencies were added" reads
        // wrong; pin the wording per count.
        if !self.added.is_empty() {
            let (dep, verb) = noun_verb_for(self.added.len());
            write!(f, "\n* {} {dep} {verb} added: ", self.added.len())?;
            let mut first = true;
            for (key, value) in &self.added {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{key}@{value}")?;
            }
        }
        if !self.removed.is_empty() {
            let (dep, verb) = noun_verb_for(self.removed.len());
            write!(f, "\n* {} {dep} {verb} removed: ", self.removed.len())?;
            let mut first = true;
            for (key, value) in &self.removed {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{key}@{value}")?;
            }
        }
        if !self.modified.is_empty() {
            let (dep, verb) = match self.modified.len() {
                1 => ("dependency", "is"),
                _ => ("dependencies", "are"),
            };
            write!(f, "\n* {} {dep} {verb} mismatched:", self.modified.len())?;
            for (key, (left, right)) in &self.modified {
                write!(f, "\n  - {key} (lockfile: {left}, manifest: {right})")?;
            }
        }
        Ok(())
    }
}

/// Singular/plural noun + past-tense verb for the `added` and
/// `removed` buckets in [`SpecDiff`]'s `Display` impl. Pulled out so
/// the arms stay readable.
fn noun_verb_for(n: usize) -> (&'static str, &'static str) {
    match n {
        1 => ("dependency", "was"),
        _ => ("dependencies", "were"),
    }
}

/// `true` when the flat-record diff is empty in all three buckets —
/// the manifest and the lockfile agree on the set of specifiers.
impl SpecDiff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}

/// Verify that lockfile-level settings the install pipeline reads
/// from `pnpm-workspace.yaml` haven't drifted since the lockfile was
/// written: `catalogs`, `overrides`, `packageExtensionsChecksum`,
/// `ignoredOptionalDependencies`, `patchedDependencies`,
/// `pnpmfileChecksum`, and the relevant `settings.*` keys (umbrella
/// [#434] slice 7).
///
/// Drift in any of these settings would otherwise require a full
/// resolution; pacquet has no resolver, so the matching action is to
/// abort the frozen install with `OutdatedLockfile`. The fields are
/// compared in the order pnpm's `getOutdatedLockfileSetting` compares
/// them, because only the *first* drifted field is reported and both
/// stacks must name the same one.
///
/// [#434]: https://github.com/pnpm/pacquet/issues/434
pub fn check_lockfile_settings(
    lockfile: &Lockfile,
    check: LockfileSettingsCheck<'_>,
) -> Result<(), StalenessReason> {
    let LockfileSettingsCheck {
        catalogs,
        overrides,
        package_extensions_checksum,
        ignored_optional_dependencies,
        patched_dependencies,
        auto_install_peers,
        dedupe_peers,
        exclude_links_from_lockfile,
        inject_workspace_packages,
        peers_suffix_max_length,
        pnpmfile_checksum,
    } = check;

    if !all_catalogs_are_up_to_date(catalogs, lockfile.catalogs.as_ref()) {
        return Err(StalenessReason::CatalogsChanged {
            lockfile: lockfile.catalogs.clone(),
            config: catalogs.clone(),
        });
    }

    let empty: HashMap<String, String> = HashMap::new();
    let lockfile_overrides: BTreeMap<String, String> = lockfile
        .overrides
        .as_ref()
        .map(|map| map.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
        .unwrap_or_default();
    let config_overrides: BTreeMap<String, String> =
        overrides.unwrap_or(&empty).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    if lockfile_overrides != config_overrides {
        return Err(StalenessReason::OverridesChanged {
            lockfile: lockfile_overrides,
            config: config_overrides,
        });
    }

    if lockfile.package_extensions_checksum.as_deref() != package_extensions_checksum {
        return Err(StalenessReason::PackageExtensionsChecksumChanged {
            lockfile: lockfile.package_extensions_checksum.clone(),
            config: package_extensions_checksum.map(str::to_string),
        });
    }

    let mut lockfile_set: Vec<String> =
        lockfile.ignored_optional_dependencies.clone().unwrap_or_default();
    let mut config_set: Vec<String> = ignored_optional_dependencies.unwrap_or(&[]).to_vec();
    lockfile_set.sort();
    config_set.sort();
    if lockfile_set != config_set {
        return Err(StalenessReason::IgnoredOptionalDependenciesChanged {
            lockfile: lockfile_set,
            config: config_set,
        });
    }

    // A changed patch file changes its hash here, which is what
    // invalidates a lockfile whose `(patch_hash=...)` depPath suffixes
    // would otherwise go stale.
    let empty_patches: BTreeMap<String, String> = BTreeMap::new();
    let lockfile_patches = lockfile.patched_dependencies.as_ref().unwrap_or(&empty_patches);
    let config_patches = patched_dependencies.unwrap_or(&empty_patches);
    if lockfile_patches != config_patches {
        return Err(StalenessReason::PatchedDependenciesChanged {
            lockfile: lockfile_patches.clone(),
            config: config_patches.clone(),
        });
    }

    // A lockfile with no `settings` block records nothing about the
    // setting it was written under, so there is nothing to compare —
    // pnpm's `lockfile.settings?.autoInstallPeers != null` guard.
    if let Some(settings) = lockfile.settings.as_ref()
        && auto_install_peers_changed(Some(settings), auto_install_peers)
    {
        return Err(StalenessReason::AutoInstallPeersChanged {
            lockfile: settings.auto_install_peers,
            config: auto_install_peers,
        });
    }

    let lockfile_dedupe_peers = recorded_dedupe_peers(lockfile.settings.as_ref());
    if lockfile_dedupe_peers != dedupe_peers {
        return Err(StalenessReason::DedupePeersChanged {
            lockfile: lockfile_dedupe_peers,
            config: dedupe_peers,
        });
    }

    if let Some(settings) = lockfile.settings.as_ref()
        && exclude_links_from_lockfile_changed(Some(settings), exclude_links_from_lockfile)
    {
        return Err(StalenessReason::ExcludeLinksFromLockfileChanged {
            lockfile: settings.exclude_links_from_lockfile,
            config: exclude_links_from_lockfile,
        });
    }

    let lockfile_peers_suffix_max_length =
        recorded_peers_suffix_max_length(lockfile.settings.as_ref());
    if lockfile_peers_suffix_max_length != peers_suffix_max_length {
        return Err(StalenessReason::PeersSuffixMaxLengthChanged {
            lockfile: lockfile_peers_suffix_max_length,
            config: peers_suffix_max_length,
        });
    }

    if let PnpmfileChecksumCheck::Current(pnpmfile_checksum) = pnpmfile_checksum
        && lockfile.pnpmfile_checksum.as_deref() != pnpmfile_checksum
    {
        return Err(StalenessReason::PnpmfileChecksumChanged {
            lockfile: lockfile.pnpmfile_checksum.clone(),
            config: pnpmfile_checksum.map(str::to_string),
        });
    }

    let lockfile_inject = recorded_inject_workspace_packages(lockfile.settings.as_ref());
    if lockfile_inject != inject_workspace_packages {
        return Err(StalenessReason::InjectWorkspacePackagesChanged {
            lockfile: lockfile_inject,
            config: inject_workspace_packages,
        });
    }

    Ok(())
}

/// Whether `settings.autoInstallPeers` drifted from what the lockfile
/// records. A lockfile with no `settings` block records nothing about
/// the setting it was written under, so there is nothing to compare.
///
/// This and its four peers below are the single definition of "this
/// lockfile setting changed", shared with the fast path that records a
/// provably inert setting change without re-resolving.
#[must_use]
pub fn auto_install_peers_changed(
    recorded: Option<&crate::LockfileSettings>,
    auto_install_peers: bool,
) -> bool {
    recorded.is_some_and(|settings| settings.auto_install_peers != auto_install_peers)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn exclude_links_from_lockfile_changed(
    recorded: Option<&crate::LockfileSettings>,
    exclude_links_from_lockfile: bool,
) -> bool {
    recorded
        .is_some_and(|settings| settings.exclude_links_from_lockfile != exclude_links_from_lockfile)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn recorded_dedupe_peers(recorded: Option<&crate::LockfileSettings>) -> bool {
    recorded.and_then(|settings| settings.dedupe_peers).unwrap_or(false)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn recorded_peers_suffix_max_length(recorded: Option<&crate::LockfileSettings>) -> u64 {
    recorded
        .and_then(|settings| settings.peers_suffix_max_length)
        .unwrap_or(crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn recorded_inject_workspace_packages(recorded: Option<&crate::LockfileSettings>) -> bool {
    recorded.is_some_and(|settings| settings.inject_workspace_packages)
}

fn all_catalogs_are_up_to_date(
    catalogs_config: &Catalogs,
    snapshot: Option<&crate::CatalogSnapshots>,
) -> bool {
    snapshot.iter().flat_map(|catalogs| catalogs.iter()).all(|(catalog_name, catalog)| {
        catalog.iter().all(|(alias, entry)| {
            catalogs_config
                .get(catalog_name)
                .and_then(|catalog| catalog.get(alias))
                .is_some_and(|specifier| dependency_specifiers_equal(&entry.specifier, specifier))
        })
    })
}

/// Verify the on-disk `package.json` is still satisfied by the
/// lockfile's importer entry for the same project. Returns `Ok(())`
/// when the lockfile is up-to-date; returns `Err(StalenessReason)`
/// describing the first detected mismatch otherwise.
///
/// What is checked (in order, short-circuiting on the first failure):
///
/// 1. Flat-record specifier diff against `devDependencies ∪
///    dependencies ∪ optionalDependencies` (∪ the auto-installed
///    peers below). Catches added / removed / modified deps in one
///    bucket.
/// 2. `publishDirectory` vs `publishConfig.directory`.
/// 3. `dependenciesMeta` equality.
/// 4. Per-field name-set, per-dep specifier, and resolved-version
///    checks. Catches
///    same-name-same-specifier-but-listed-under-different-field
///    drift the flat-record diff doesn't see, plus broken lockfiles
///    whose resolved semver no longer satisfies the recorded range.
///
/// When `auto_install_peers` is set (pnpm's default), every peer
/// dependency missing from the regular dependency fields is folded
/// into `dependencies` for the comparison, matching how pnpm
/// materializes those peers into the importer's `dependencies` in the
/// lockfile. Without this, a peer-only dependency would be misread as
/// a lockfile entry the manifest removed (see `auto_installed_peer_deps`).
///
/// Still unsupported: `excludeLinksFromLockfile` (`link:` resolutions
/// aren't modeled yet). Non-semver resolutions such as file and
/// tarball dependencies are excluded from the resolved-version check.
pub fn satisfies_package_manifest(
    importer: &ProjectSnapshot,
    manifest: &PackageManifest,
    auto_install_peers: bool,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> Result<(), StalenessReason> {
    let folded_peers = auto_installed_peer_deps(manifest, auto_install_peers);

    // Phase 1: flat-record diff against the manifest's union of
    // dependency fields. Compares the importer's specifiers to the
    // manifest's existing deps (devs + prod + optional flattened
    // together, plus the auto-installed peers).
    let mut manifest_specs = flat_manifest_specs(manifest, is_ignored_optional);
    manifest_specs
        .extend(folded_peers.iter().map(|(name, spec)| ((*name).to_string(), (*spec).to_string())));
    let importer_specs = flat_importer_specs(importer);
    let diff = diff_flat_records(&importer_specs, &manifest_specs);
    if !diff.is_empty() {
        return Err(StalenessReason::SpecifiersDiffer(diff));
    }

    // Phase 2: `publishDirectory` parity. Compares the importer's
    // `publishDirectory` to the manifest's `publishConfig.directory`
    // verbatim; pacquet's `ProjectSnapshot.publish_directory` is
    // `Option<String>` and the manifest exposes the field via the
    // raw `value()`. Two `None`s match; anything else mismatched
    // fails the check.
    let manifest_publish_dir = manifest
        .value()
        .get("publishConfig")
        .and_then(|p| p.get("directory"))
        .and_then(|d| d.as_str())
        .map(str::to_owned);
    if importer.publish_directory != manifest_publish_dir {
        return Err(StalenessReason::PublishDirectoryMismatch {
            lockfile: importer.publish_directory.clone(),
            manifest: manifest_publish_dir,
        });
    }

    // Phase 3: `dependenciesMeta` parity. JSON-equality of the two
    // maps (or both absent), so an absent map and an empty map are
    // equivalent.
    let manifest_meta = manifest.value().get("dependenciesMeta");
    let importer_meta = importer.dependencies_meta.as_ref();
    if !dependencies_meta_equal(importer_meta, manifest_meta) {
        return Err(StalenessReason::DependenciesMetaMismatch {
            lockfile: importer_meta
                .map_or_else(|| "{}".to_string(), std::string::ToString::to_string),
            manifest: manifest_meta
                .map_or_else(|| "{}".to_string(), std::string::ToString::to_string),
        });
    }

    // Phase 4: per-field name-set + specifier match. The auto-installed
    // peers join `dependencies`, so they count toward the prod name-set
    // used for both the dev-field precedence filter and this field's
    // own comparison.
    let mut manifest_prod: BTreeMap<&str, &str> = manifest
        .dependencies([DependencyGroup::Prod])
        .filter(|(name, _)| !is_ignored_optional(name))
        .collect();
    manifest_prod.extend(folded_peers.iter().map(|(name, spec)| (*name, *spec)));
    let manifest_optional: BTreeMap<&str, &str> = manifest
        .dependencies([DependencyGroup::Optional])
        .filter(|(name, _)| !is_ignored_optional(name))
        .collect();
    for field in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        let field_name = <&'static str>::from(field);
        let mut manifest_field: BTreeMap<&str, &str> = manifest
            .dependencies([field])
            .filter(|(name, _)| {
                !matches!(field, DependencyGroup::Prod | DependencyGroup::Optional)
                    || !is_ignored_optional(name)
            })
            .filter(|(name, _)| match field {
                DependencyGroup::Dev => {
                    !manifest_prod.contains_key(*name) && !manifest_optional.contains_key(*name)
                }
                DependencyGroup::Prod => !manifest_optional.contains_key(*name),
                DependencyGroup::Optional | DependencyGroup::Peer => true,
            })
            .collect();
        if matches!(field, DependencyGroup::Prod) {
            manifest_field.extend(folded_peers.iter().map(|(name, spec)| (*name, *spec)));
        }
        let importer_field = importer.get_map_by_group(field);

        // Every manifest entry must have a matching importer entry
        // in the *same* field with the same specifier.
        for (name, manifest_spec) in &manifest_field {
            let parsed = crate::PkgName::parse(*name).ok();
            let importer_spec = parsed
                .as_ref()
                .and_then(|name| importer_field.and_then(|map| map.get(name)))
                .map(|spec| spec.specifier.as_str());
            match importer_spec {
                Some(spec) if dependency_specifiers_equal(spec, manifest_spec) => {}
                Some(spec) => {
                    return Err(StalenessReason::DepSpecifierMismatch {
                        field: field_name,
                        name: (*name).to_string(),
                        lockfile: spec.to_string(),
                        manifest: (*manifest_spec).to_string(),
                    });
                }
                None => {
                    return Err(StalenessReason::DepSpecifierMismatch {
                        field: field_name,
                        name: (*name).to_string(),
                        lockfile: "(absent)".to_string(),
                        manifest: (*manifest_spec).to_string(),
                    });
                }
            }
            let Some(importer_dep) =
                parsed.as_ref().and_then(|name| importer_field.and_then(|map| map.get(name)))
            else {
                continue;
            };
            let (Some(version), Ok(range)) = (
                importer_dep.version.ver_peer().and_then(|version| version.version_semver()),
                manifest_spec.parse::<node_semver::Range>(),
            ) else {
                continue;
            };
            if !range.satisfies(version) {
                return Err(StalenessReason::ResolutionDoesNotSatisfy {
                    name: (*name).to_string(),
                    version: version.to_string(),
                    range: (*manifest_spec).to_string(),
                });
            }
        }

        // Every importer entry in this field must also exist in the
        // manifest's same field (post-precedence-filter). Catches
        // the inverse of the loop above (lockfile lists a dep here
        // that the manifest moved to a different field).
        if let Some(importer_map) = importer_field {
            for (name, spec) in importer_map {
                if !manifest_field.contains_key(name.to_string().as_str()) {
                    return Err(StalenessReason::DepSpecifierMismatch {
                        field: field_name,
                        name: name.to_string(),
                        lockfile: spec.specifier.clone(),
                        manifest: "(absent)".to_string(),
                    });
                }
            }
        }
    }

    Ok(())
}

/// Two `dependenciesMeta` maps are equal when both are absent / empty
/// or both render to the same JSON.
fn dependencies_meta_equal(
    importer: Option<&serde_json::Value>,
    manifest: Option<&serde_json::Value>,
) -> bool {
    fn is_empty_object(value: Option<&serde_json::Value>) -> bool {
        match value {
            None => true,
            Some(serde_json::Value::Object(map)) => map.is_empty(),
            Some(serde_json::Value::Null) => true,
            _ => false,
        }
    }
    match (importer, manifest) {
        (None, None) => true,
        (a, b) if is_empty_object(a) && is_empty_object(b) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Peer dependencies that `auto-install-peers` materializes into the
/// importer's `dependencies`: every `peerDependencies` entry whose name
/// isn't already declared in `dependencies`, `devDependencies`, or
/// `optionalDependencies`. Empty when `auto_install_peers` is off, which
/// restores the plain manifest-vs-lockfile comparison.
///
/// Mirrors pnpm's `omit(Object.keys(existingDeps), pkg.peerDependencies)`
/// fold in `satisfiesPackageManifest`: peers already declared in a
/// regular field keep that field's specifier, so only the peer-only
/// entries are surfaced here.
pub(crate) fn auto_installed_peer_deps(
    manifest: &PackageManifest,
    auto_install_peers: bool,
) -> BTreeMap<&str, &str> {
    if !auto_install_peers {
        return BTreeMap::new();
    }
    let declared: BTreeSet<&str> = manifest
        .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
        .map(|(name, _)| name)
        .collect();
    manifest
        .dependencies([DependencyGroup::Peer])
        .filter(|(name, _)| !declared.contains(name))
        .collect()
}

/// Build the manifest's `devDependencies ∪ dependencies ∪
/// optionalDependencies` flat-record. Manifest fields are read in
/// dev → prod → optional order, but the order
/// is irrelevant for the diff since duplicates resolve to the same
/// specifier anyway — if two fields list the same name with different
/// specifiers the manifest is invalid and pacquet would have rejected
/// it earlier.
fn flat_manifest_specs(
    manifest: &PackageManifest,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        for (name, spec) in manifest.dependencies([group]) {
            if matches!(group, DependencyGroup::Prod | DependencyGroup::Optional)
                && is_ignored_optional(name)
            {
                continue;
            }
            out.insert(name.to_string(), spec.to_string());
        }
    }
    out
}

/// Build the importer's flat-record from its three dependency maps.
/// The inline-specifier shape of v9 lockfiles means each entry
/// already carries its `specifier` field; no top-level
/// `importer.specifiers` map is consulted (that's a v6/v7 shape that
/// pacquet's `ProjectSnapshot` still models for serde compatibility
/// but doesn't use here).
fn flat_importer_specs(importer: &ProjectSnapshot) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        if let Some(map) = importer.get_map_by_group(group) {
            for (name, spec) in map {
                out.insert(name.to_string(), spec.specifier.clone());
            }
        }
    }
    out
}

/// Bucket entries from two flat records into added/removed/modified.
/// `removed` is what's in `lockfile_specs` but missing from `manifest_specs`,
/// `added` is the inverse, `modified` are keys present in both but with
/// different values.
fn diff_flat_records(
    lockfile_specs: &BTreeMap<String, String>,
    manifest_specs: &BTreeMap<String, String>,
) -> SpecDiff {
    let lhs_keys: BTreeSet<&String> = lockfile_specs.keys().collect();
    let rhs_keys: BTreeSet<&String> = manifest_specs.keys().collect();
    let mut diff = SpecDiff::default();
    for k in lhs_keys.difference(&rhs_keys) {
        diff.removed.insert((**k).clone(), lockfile_specs[*k].clone());
    }
    for k in rhs_keys.difference(&lhs_keys) {
        diff.added.insert((**k).clone(), manifest_specs[*k].clone());
    }
    for k in lhs_keys.intersection(&rhs_keys) {
        let lhs_spec = &lockfile_specs[*k];
        let rhs_spec = &manifest_specs[*k];
        if !dependency_specifiers_equal(lhs_spec, rhs_spec) {
            diff.modified.insert((**k).clone(), (lhs_spec.clone(), rhs_spec.clone()));
        }
    }
    diff
}

fn dependency_specifiers_equal(left: &str, right: &str) -> bool {
    left == right || git_specifiers_are_equivalent(left, right)
}

#[cfg(test)]
mod tests;
