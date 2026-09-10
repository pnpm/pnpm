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

pub(crate) use manifest::auto_installed_peer_deps;
pub use manifest::satisfies_package_manifest;

use crate::{Lockfile, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec};
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

    /// Whether workspace links use `publishDirectory` differs from the
    /// manifest's effective `publishConfig.linkDirectory` value.
    #[display(
        r#""linkDirectory" in the lockfile ({lockfile}) doesn't match "publishConfig.linkDirectory" in package.json ({manifest})"#
    )]
    LinkDirectoryMismatch { lockfile: bool, manifest: bool },

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
            | StalenessReason::LinkDirectoryMismatch { .. }
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
        write_spec_bucket(f, "added", &self.added)?;
        write_spec_bucket(f, "removed", &self.removed)?;
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

/// One `added` / `removed` bucket of [`SpecDiff`]'s `Display` impl.
///
/// Singular/plural matters here: the diff is rendered into
/// `ERR_PNPM_OUTDATED_LOCKFILE` CI output, which users see and may quote in
/// issues. "1 dependencies were added" reads wrong; the wording is pinned
/// per count.
fn write_spec_bucket(
    f: &mut std::fmt::Formatter<'_>,
    what: &str,
    specs: &BTreeMap<String, String>,
) -> std::fmt::Result {
    if specs.is_empty() {
        return Ok(());
    }
    let (dep, verb) = noun_verb_for(specs.len());
    write!(f, "\n* {} {dep} {verb} {what}: ", specs.len())?;
    let rendered: Vec<String> = specs.iter().map(|(key, value)| format!("{key}@{value}")).collect();
    write!(f, "{}", rendered.join(", "))
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
    check_recorded_config(lockfile, &check)?;
    check_recorded_settings(lockfile, &check)
}

/// The config inputs the lockfile records verbatim: catalogs, overrides,
/// package extensions, ignored optional dependencies and patches.
fn check_recorded_config(
    lockfile: &Lockfile,
    check: &LockfileSettingsCheck<'_>,
) -> Result<(), StalenessReason> {
    if !all_catalogs_are_up_to_date(check.catalogs, lockfile.catalogs.as_ref()) {
        return Err(StalenessReason::CatalogsChanged {
            lockfile: lockfile.catalogs.clone(),
            config: check.catalogs.clone(),
        });
    }

    check_overrides(lockfile, check.overrides)?;

    if lockfile.package_extensions_checksum.as_deref() != check.package_extensions_checksum {
        return Err(StalenessReason::PackageExtensionsChecksumChanged {
            lockfile: lockfile.package_extensions_checksum.clone(),
            config: check.package_extensions_checksum.map(str::to_string),
        });
    }

    let mut lockfile_set: Vec<String> =
        lockfile.ignored_optional_dependencies.clone().unwrap_or_default();
    let mut config_set: Vec<String> = check.ignored_optional_dependencies.unwrap_or(&[]).to_vec();
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
    let config_patches = check.patched_dependencies.unwrap_or(&empty_patches);
    if lockfile_patches != config_patches {
        return Err(StalenessReason::PatchedDependenciesChanged {
            lockfile: lockfile_patches.clone(),
            config: config_patches.clone(),
        });
    }
    Ok(())
}

fn check_overrides(
    lockfile: &Lockfile,
    config_overrides: Option<&HashMap<String, String>>,
) -> Result<(), StalenessReason> {
    let lockfile_overrides: BTreeMap<String, String> = lockfile
        .overrides
        .as_ref()
        .map(|map| map.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
        .unwrap_or_default();
    let config_overrides: BTreeMap<String, String> = config_overrides
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if lockfile_overrides != config_overrides {
        return Err(StalenessReason::OverridesChanged {
            lockfile: lockfile_overrides,
            config: config_overrides,
        });
    }
    Ok(())
}

/// The `settings:` block plus the pnpmfile checksum. A lockfile with no
/// `settings` block records nothing about the settings it was written under,
/// so there is nothing to compare — pnpm's
/// `lockfile.settings?.autoInstallPeers != null` guard.
fn check_recorded_settings(
    lockfile: &Lockfile,
    check: &LockfileSettingsCheck<'_>,
) -> Result<(), StalenessReason> {
    let settings = lockfile.settings.as_ref();
    if let Some(settings) = settings
        && auto_install_peers_changed(Some(settings), check.auto_install_peers)
    {
        return Err(StalenessReason::AutoInstallPeersChanged {
            lockfile: settings.auto_install_peers,
            config: check.auto_install_peers,
        });
    }

    let lockfile_dedupe_peers = recorded_dedupe_peers(settings);
    if lockfile_dedupe_peers != check.dedupe_peers {
        return Err(StalenessReason::DedupePeersChanged {
            lockfile: lockfile_dedupe_peers,
            config: check.dedupe_peers,
        });
    }

    if let Some(settings) = settings
        && exclude_links_from_lockfile_changed(Some(settings), check.exclude_links_from_lockfile)
    {
        return Err(StalenessReason::ExcludeLinksFromLockfileChanged {
            lockfile: settings.exclude_links_from_lockfile,
            config: check.exclude_links_from_lockfile,
        });
    }

    let lockfile_peers_suffix_max_length = recorded_peers_suffix_max_length(settings);
    if lockfile_peers_suffix_max_length != check.peers_suffix_max_length {
        return Err(StalenessReason::PeersSuffixMaxLengthChanged {
            lockfile: lockfile_peers_suffix_max_length,
            config: check.peers_suffix_max_length,
        });
    }

    check_pnpmfile_checksum(lockfile, &check.pnpmfile_checksum)?;

    let lockfile_inject = recorded_inject_workspace_packages(settings);
    if lockfile_inject != check.inject_workspace_packages {
        return Err(StalenessReason::InjectWorkspacePackagesChanged {
            lockfile: lockfile_inject,
            config: check.inject_workspace_packages,
        });
    }

    Ok(())
}

fn check_pnpmfile_checksum(
    lockfile: &Lockfile,
    checksum: &PnpmfileChecksumCheck<'_>,
) -> Result<(), StalenessReason> {
    if let PnpmfileChecksumCheck::Current(pnpmfile_checksum) = checksum
        && lockfile.pnpmfile_checksum.as_deref() != *pnpmfile_checksum
    {
        return Err(StalenessReason::PnpmfileChecksumChanged {
            lockfile: lockfile.pnpmfile_checksum.clone(),
            config: pnpmfile_checksum.map(str::to_string),
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

#[cfg(test)]
mod tests;

mod manifest;

use manifest::dependency_specifiers_equal;
