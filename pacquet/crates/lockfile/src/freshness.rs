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
//! This module ports upstream's
//! [`satisfiesPackageManifest`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts):
//! a per-importer structural comparison that returns the first
//! mismatch (if any) as a typed [`StalenessReason`]. The frozen-
//! lockfile dispatcher surfaces this as `ERR_PNPM_OUTDATED_LOCKFILE`,
//! matching upstream's CI-correctness contract.

use crate::{Lockfile, ProjectSnapshot};
use derive_more::{Display, Error};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Why an importer's lockfile entry doesn't satisfy the on-disk
/// `package.json`. Mirrors the discriminated cases upstream's
/// `satisfiesPackageManifest` returns as `detailedReason` strings,
/// but as a typed enum so callers can match on the discriminant
/// without parsing format strings, and tests can assert against the
/// shape rather than the wording.
#[derive(Debug, Display, Error, PartialEq)]
#[non_exhaustive]
pub enum StalenessReason {
    /// The lockfile has no `importers["."]` (or whatever id) entry,
    /// so we can't even start the comparison. Mirrors upstream's
    /// "no importer" reason at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L20>.
    #[display(r#"the lockfile has no `importers["{importer_id}"]` entry"#)]
    NoImporter { importer_id: String },

    /// The flat union of `dependencies ∪ devDependencies ∪
    /// optionalDependencies` from the manifest doesn't match the
    /// per-dep specifiers recorded in the importer entry. Mirrors
    /// upstream's "specifiers in the lockfile don't match specifiers
    /// in package.json" reason at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L45>.
    #[display("specifiers in the lockfile don't match specifiers in package.json:{_0}")]
    SpecifiersDiffer(#[error(not(source))] SpecDiff),

    /// `publishDirectory` on the importer entry doesn't match
    /// `publishConfig.directory` on the manifest. Mirrors upstream's
    /// `publishDirectory` mismatch at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L51>.
    #[display(
        "`publishDirectory` in the lockfile ({lockfile:?}) doesn't match `publishConfig.directory` in package.json ({manifest:?})"
    )]
    PublishDirectoryMismatch { lockfile: Option<String>, manifest: Option<String> },

    /// `dependenciesMeta` on the importer doesn't match
    /// `dependenciesMeta` on the manifest. Mirrors upstream's check at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L57>.
    #[display(
        "importer dependencies meta ({lockfile}) doesn't match package manifest dependencies meta ({manifest})"
    )]
    DependenciesMetaMismatch { lockfile: String, manifest: String },

    /// The recorded specifier for one dep diverges from the manifest's
    /// specifier for the same dep. Mirrors upstream's "importer
    /// dependencies.X specifier Y don't match package manifest
    /// specifier (Z)" at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L97>.
    #[display(
        "importer {field}.{name} specifier {lockfile:?} doesn't match package manifest specifier ({manifest:?})"
    )]
    DepSpecifierMismatch { field: &'static str, name: String, lockfile: String, manifest: String },

    /// The lockfile's `ignoredOptionalDependencies` (sorted) differs
    /// from the current install's `Config::ignored_optional_dependencies`
    /// (sorted). Mirrors upstream's
    /// [`getOutdatedLockfileSetting.ts:58-60`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L58-L60):
    /// upstream returns `'ignoredOptionalDependencies'` from the
    /// settings checker and `needsFullResolution` flips on. Pacquet
    /// has no resolver, so the matching action is to surface this as
    /// `OutdatedLockfile`. Both values are returned sorted so the
    /// error message reads stably in CI logs.
    #[display(
        "`ignoredOptionalDependencies` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    IgnoredOptionalDependenciesChanged { lockfile: Vec<String>, config: Vec<String> },

    /// The lockfile's `overrides` map doesn't match the current
    /// install's `Config::overrides`. Mirrors upstream's
    /// [`getOutdatedLockfileSetting.ts:50-52`](https://github.com/pnpm/pnpm/blob/606f53e78f/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L50-L52):
    /// upstream returns `'overrides'` and `needsFullResolution` flips
    /// on. Pacquet has no resolver, so the matching action is to
    /// surface this as `OutdatedLockfile`. Both values are normalized
    /// into a `BTreeMap` so the order-insensitive comparison upstream
    /// runs through `equals(lockfile.overrides ?? {}, overrides ?? {})`
    /// is preserved, and the rendered error reads stably.
    #[display(
        "`overrides` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    OverridesChanged { lockfile: BTreeMap<String, String>, config: BTreeMap<String, String> },

    /// The lockfile's `settings.injectWorkspacePackages` differs from
    /// the current install's `Config::inject_workspace_packages`.
    /// Mirrors upstream's
    /// [`getOutdatedLockfileSetting.ts:80-82`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L80-L82):
    /// the gate normalizes both sides through `Boolean(...)` so an
    /// absent setting equals an explicit `false`. Pacquet has no
    /// resolver, so the matching action is to surface this as
    /// `OutdatedLockfile`.
    #[display(
        "`injectWorkspacePackages` in the lockfile ({lockfile}) doesn't match the current config ({config})"
    )]
    InjectWorkspacePackagesChanged { lockfile: bool, config: bool },

    /// `settings.peersSuffixMaxLength` in the lockfile differs from
    /// the value the current install would use. Mirrors upstream's
    /// [`getOutdatedLockfileSetting.ts`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts):
    /// an unset field in the lockfile is treated as the default
    /// (1000), so drift is "recorded value (or default) doesn't equal
    /// the current config's value".
    #[display(
        "`peersSuffixMaxLength` in the lockfile ({lockfile}) doesn't match the current config ({config})"
    )]
    PeersSuffixMaxLengthChanged { lockfile: u64, config: u64 },

    /// The lockfile's `packageExtensionsChecksum` doesn't match the
    /// checksum derived from the current install's
    /// `Config::package_extensions`. Mirrors upstream's
    /// [`getOutdatedLockfileSetting.ts:53-55`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L53-L55):
    /// upstream returns `'packageExtensionsChecksum'` and
    /// `needsFullResolution` flips on. Pacquet has no resolver, so the
    /// matching action is to surface this as `OutdatedLockfile`. Both
    /// values are the prefixed `sha256-…` strings the writer emits.
    #[display(
        "`packageExtensionsChecksum` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    PackageExtensionsChecksumChanged { lockfile: Option<String>, config: Option<String> },

    /// The lockfile's `patchedDependencies` (key → patch-file hash)
    /// doesn't match the map the current install would write. Mirrors
    /// upstream's
    /// [`getOutdatedLockfileSetting.ts:61-63`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L61-L63):
    /// upstream returns `'patchedDependencies'` and `needsFullResolution`
    /// flips on. Pacquet has no resolver, so the matching action is to
    /// surface this as `OutdatedLockfile`. A changed patch file changes
    /// its hash here, which is what catches an edited patch whose
    /// `(patch_hash=...)` depPath suffix would otherwise go stale. Both
    /// values are normalized into a `BTreeMap` so the comparison is
    /// order-insensitive (matching upstream's Ramda `equals`).
    #[display(
        "`patchedDependencies` in the lockfile ({lockfile:?}) doesn't match the current config ({config:?})"
    )]
    PatchedDependenciesChanged {
        lockfile: BTreeMap<String, String>,
        config: BTreeMap<String, String>,
    },
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
}

impl std::fmt::Display for SpecDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
/// from `pnpm-workspace.yaml` haven't drifted since the lockfile
/// was written. Today this covers `overrides`,
/// `packageExtensionsChecksum`, `ignoredOptionalDependencies`,
/// `patchedDependencies`, and the relevant `settings.*` keys (umbrella
/// [#434] slice 7); the variants below will grow as more upstream
/// settings land (`catalogs`, `pnpmfileChecksum`, etc.).
///
/// Mirrors upstream's
/// [`getOutdatedLockfileSetting`](https://github.com/pnpm/pnpm/blob/606f53e78f/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts).
/// Upstream uses the return value to flip `needsFullResolution`;
/// pacquet has no resolver, so the matching action is to abort the
/// frozen install with `OutdatedLockfile`. The check ordering here
/// matches upstream's so the *first* drifted field is reported on
/// both sides — which matters for tests and for CI logs that quote
/// the reason verbatim.
///
/// [#434]: https://github.com/pnpm/pacquet/issues/434
pub fn check_lockfile_settings(
    lockfile: &Lockfile,
    overrides: Option<&HashMap<String, String>>,
    package_extensions_checksum: Option<&str>,
    ignored_optional_dependencies: Option<&[String]>,
    patched_dependencies: Option<&BTreeMap<String, String>>,
    inject_workspace_packages: bool,
    peers_suffix_max_length: u64,
) -> Result<(), StalenessReason> {
    // Upstream checks `overrides` before `ignoredOptionalDependencies`,
    // so an install that changed both surfaces the overrides drift
    // first — preserving that for parity with pnpm error reports.
    // `BTreeMap` normalizes ordering so the order-insensitive `equals`
    // upstream uses lines up with `==` here, and the `Display` impl
    // renders the diff stably.
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

    // Upstream checks `packageExtensionsChecksum` next, before
    // `ignoredOptionalDependencies`. The check is `!==` on the two
    // optional strings: absent on both sides is equivalent (no
    // extensions ever configured), and absent vs. present is drift.
    if lockfile.package_extensions_checksum.as_deref() != package_extensions_checksum {
        return Err(StalenessReason::PackageExtensionsChecksumChanged {
            lockfile: lockfile.package_extensions_checksum.clone(),
            config: package_extensions_checksum.map(str::to_string),
        });
    }

    // Comparison is order-insensitive — upstream sorts both sides
    // before calling Ramda's `equals`. Empty `None` and empty `[]`
    // are equivalent (matches upstream's `?? []` default on both
    // sides).
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

    // Upstream checks `patchedDependencies` right after
    // `ignoredOptionalDependencies` via
    // `!equals(lockfile.patchedDependencies ?? {}, patchedDependencies ?? {})`.
    // Both maps are already key-sorted (`BTreeMap`), so `==` reproduces
    // the order-insensitive `equals`; absent on either side normalizes
    // to an empty map. A changed patch file changes its hash here, so
    // this is what invalidates a lockfile whose `(patch_hash=...)` depPath
    // suffixes would otherwise go stale.
    let empty_patches: BTreeMap<String, String> = BTreeMap::new();
    let lockfile_patches = lockfile.patched_dependencies.as_ref().unwrap_or(&empty_patches);
    let config_patches = patched_dependencies.unwrap_or(&empty_patches);
    if lockfile_patches != config_patches {
        return Err(StalenessReason::PatchedDependenciesChanged {
            lockfile: lockfile_patches.clone(),
            config: config_patches.clone(),
        });
    }

    // `Boolean(lockfile.settings?.injectWorkspacePackages) !==
    // Boolean(injectWorkspacePackages)` at upstream's
    // [`getOutdatedLockfileSetting.ts:80-82`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L80-L82).
    // Pacquet's wire format omits the key when `false` (see
    // [`LockfileSettings`]'s `skip_serializing_if`), so an absent
    // settings block or a missing key both deserialize as `false`
    // here and the `Boolean(...)` normalization is automatic.
    let lockfile_inject =
        lockfile.settings.as_ref().is_some_and(|settings| settings.inject_workspace_packages);
    if lockfile_inject != inject_workspace_packages {
        return Err(StalenessReason::InjectWorkspacePackagesChanged {
            lockfile: lockfile_inject,
            config: inject_workspace_packages,
        });
    }

    // An unset `peersSuffixMaxLength` in the lockfile means the writer
    // used the default (1000) — pnpm strips the field on serialization
    // when it equals the default. So drift here is "lockfile's
    // recorded-or-default value != current config's value".
    let lockfile_peers_suffix_max_length = lockfile
        .settings
        .as_ref()
        .and_then(|s| s.peers_suffix_max_length)
        .unwrap_or(crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH);
    if lockfile_peers_suffix_max_length != peers_suffix_max_length {
        return Err(StalenessReason::PeersSuffixMaxLengthChanged {
            lockfile: lockfile_peers_suffix_max_length,
            config: peers_suffix_max_length,
        });
    }

    Ok(())
}

/// Verify the on-disk `package.json` is still satisfied by the
/// lockfile's importer entry for the same project. Returns `Ok(())`
/// when the lockfile is up-to-date; returns `Err(StalenessReason)`
/// describing the first detected mismatch otherwise.
///
/// Single-importer only today (pacquet doesn't have workspace support
/// — see [#431]). Callers thread the root importer entry directly.
///
/// What is checked (in order, short-circuiting on the first failure):
///
/// 1. Flat-record specifier diff against `devDependencies ∪
///    dependencies ∪ optionalDependencies`. Catches added / removed /
///    modified deps in one bucket.
/// 2. `publishDirectory` vs `publishConfig.directory`.
/// 3. `dependenciesMeta` equality.
/// 4. Per-field name-set check and per-dep specifier match. Catches
///    same-name-same-specifier-but-listed-under-different-field
///    drift the flat-record diff doesn't see.
///
/// Mirrors upstream's
/// [`satisfiesPackageManifest`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts).
/// Scoped to what pacquet supports today: no catalogs (#?), no
/// `auto-install-peers` pre-pass (pacquet has no separate
/// auto-install-peers mode), no `excludeLinksFromLockfile` (`link:`
/// resolutions aren't supported yet — [#431] territory), and no
/// version-range-satisfies check (covered in pnpm's
/// `localTarballDepsAreUpToDate` for file: / tarball deps; out of
/// scope here).
///
/// [#431]: https://github.com/pnpm/pacquet/issues/431
pub fn satisfies_package_manifest(
    importer: &ProjectSnapshot,
    manifest: &PackageManifest,
    importer_id: &str,
    is_ignored_optional: &dyn Fn(&str) -> bool,
) -> Result<(), StalenessReason> {
    let _ = importer_id; // reserved for the multi-importer path once <https://github.com/pnpm/pacquet/issues/431> lands.

    // Phase 1: flat-record diff against the manifest's union of
    // dependency fields. Matches the upstream
    // `_satisfiesPackageManifest(importer, manifest).satisfies` gate
    // that compares `importer.specifiers` to `existingDeps` (devs +
    // prod + optional flattened together).
    let manifest_specs = flat_manifest_specs(manifest, is_ignored_optional);
    let importer_specs = flat_importer_specs(importer);
    let diff = diff_flat_records(&importer_specs, &manifest_specs);
    if !diff.is_empty() {
        return Err(StalenessReason::SpecifiersDiffer(diff));
    }

    // Phase 2: `publishDirectory` parity. Upstream compares
    // `importer.publishDirectory` to `pkg.publishConfig?.directory`
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
    // maps (or both absent). Upstream uses Ramda's `equals` with
    // `?? {}` on both sides, so an absent map and an empty map are
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

    // Phase 4: per-field name-set + specifier match. The flat-record
    // diff catches added/removed/modified specifiers across the
    // *union*, but doesn't catch the case where a dep keeps its
    // specifier but moves between fields (e.g. `react` moved from
    // `dependencies` to `devDependencies`) — the union stays the
    // same in both. Run the per-field comparison unconditionally
    // here to catch that and the same-cardinality cross-field-swap
    // case (lockfile prod={a}, dev={b} vs manifest prod={b}, dev={a}).
    //
    // A dep listed in *multiple* manifest fields is counted only in
    // the highest-precedence one: `optionalDependencies` >
    // `dependencies` > `devDependencies`. Matches upstream's
    // `pkgDepNames` filter at
    // <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L69-L84>.
    // Without this, a manifest with the same dep in both `deps` and
    // `devDeps` would fail the `devDeps` check even though the
    // lockfile records it under `deps` only.
    let manifest_prod: BTreeMap<&str, &str> = manifest
        .dependencies([DependencyGroup::Prod])
        .filter(|(name, _)| !is_ignored_optional(name))
        .collect();
    let manifest_optional: BTreeMap<&str, &str> = manifest
        .dependencies([DependencyGroup::Optional])
        .filter(|(name, _)| !is_ignored_optional(name))
        .collect();
    for field in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        let field_name = <&'static str>::from(field);
        let manifest_field: BTreeMap<&str, &str> = manifest
            .dependencies([field])
            // `ignoredOptionalDependencies` (umbrella <https://github.com/pnpm/pacquet/issues/434> slice 7):
            // upstream's read-package-hook strips matching entries
            // from `optionalDependencies` AND `dependencies` before
            // the resolver sees the manifest, so the lockfile never
            // carries them. `devDependencies` is intentionally
            // untouched by the hook — see
            // <https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts>:
            // the hook iterates `optionalDependencies` keys and
            // deletes from `optionalDependencies` plus
            // `dependencies` only.
            .filter(|(name, _)| {
                !matches!(field, DependencyGroup::Prod | DependencyGroup::Optional)
                    || !is_ignored_optional(name)
            })
            .filter(|(name, _)| match field {
                // `dev` deps are dropped if also listed in `prod` or
                // `optional`. `prod` deps are dropped if also in
                // `optional`. `optional` always wins.
                DependencyGroup::Dev => {
                    !manifest_prod.contains_key(*name) && !manifest_optional.contains_key(*name)
                }
                DependencyGroup::Prod => !manifest_optional.contains_key(*name),
                DependencyGroup::Optional | DependencyGroup::Peer => true,
            })
            .collect();
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
                Some(spec) if spec == *manifest_spec => continue,
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
/// or both render to the same JSON. Matches upstream's `equals(pkg
/// .dependenciesMeta ?? {}, importer.dependenciesMeta ?? {})` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/satisfiesPackageManifest.ts#L56-L58>.
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

/// Build the manifest's `devDependencies ∪ dependencies ∪
/// optionalDependencies` flat-record. Manifest fields are read in the
/// same order upstream applies (dev → prod → optional), but the order
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
            // `ignoredOptionalDependencies` filter — only applies to
            // `Prod` and `Optional`, matching upstream's
            // [`createOptionalDependenciesRemover`](https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts).
            // The hook iterates `optionalDependencies` and deletes
            // matches from there AND from `dependencies`, but
            // leaves `devDependencies` untouched. Mirroring that
            // exactly: the same name listed in `devDependencies`
            // is kept here so the lockfile-side dev entry doesn't
            // falsely surface as drift.
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
/// Mirrors upstream's
/// [`diffFlatRecords`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/diffFlatRecords.ts):
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
        if lhs_spec != rhs_spec {
            diff.modified.insert((**k).clone(), (lhs_spec.clone(), rhs_spec.clone()));
        }
    }
    diff
}

#[cfg(test)]
mod tests;
