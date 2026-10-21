//! The settings part of the freshness gate. A rebuild may narrow it to
//! the patches: it builds the locked packages without resolving them, so
//! of the settings only the patches decide what it builds.

use super::{
    super::{Config, Lockfile},
    FreshnessCheckError, FreshnessScope, LockfileFreshnessInputs,
};
use crate::RebuildOptions;
use pnpm_catalogs_types::Catalogs;
use pnpm_config_parse_overrides::VersionOverride;
use pnpm_lockfile::PnpmfileChecksumCheck;

impl LockfileFreshnessInputs<'_, '_> {
    /// Narrows the settings check to the lockfile's patches when `rebuild`
    /// asks for it. See [`RebuildOptions::check_lockfile_patches_only`].
    pub(crate) fn for_rebuild(self, rebuild: Option<&RebuildOptions>) -> Self {
        let patches_only = rebuild.is_some_and(|rebuild| rebuild.check_lockfile_patches_only);
        LockfileFreshnessInputs { scope: FreshnessScope { patches_only, ..self.scope }, ..self }
    }
}

/// The settings the lockfile records must match the current ones: all of
/// them, or only the patches when [`FreshnessScope::patches_only`] is set.
///
/// Returns the overrides the manifests are checked against the lockfile
/// with: the current ones, or for a patches-only check the ones the
/// lockfile records, as the overrides are allowed to have drifted.
pub(super) async fn check_settings(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> Result<Option<Vec<VersionOverride>>, FreshnessCheckError> {
    if inputs.scope.patches_only {
        check_lockfile_patches(lockfile, inputs.config)?;
        return parse_overrides(lockfile.overrides.as_ref(), inputs.catalogs);
    }
    let parsed_overrides = parse_config_overrides(inputs.config, inputs.catalogs)?;
    let pnpmfile_checksum = pnpm_hooks::current_pnpmfile_checksum(
        inputs.pnpmfile_hook,
        lockfile.pnpmfile_checksum.as_deref(),
    )
    .await;
    check_lockfile_settings_drift(
        lockfile,
        inputs.config,
        inputs.catalogs,
        CheckLockfileSettingsDriftOptions {
            parsed_overrides: parsed_overrides.as_deref(),
            pnpmfile_checksum: super::super::pnpmfile_checksum_check(
                inputs,
                pnpmfile_checksum.as_deref(),
            ),
            dedupe_peers: inputs.config.dedupe_peers,
        },
    )?;
    Ok(parsed_overrides)
}

/// The lockfile's `patchedDependencies` must match the configured patch
/// hashes, and its `(patch_hash=...)` depPath suffixes must match them.
fn check_lockfile_patches(lockfile: &Lockfile, config: &Config) -> Result<(), FreshnessCheckError> {
    let patched_dependency_hashes =
        config.patched_dependency_hashes().map_err(FreshnessCheckError::CalcPatchHashes)?;
    pnpm_lockfile::check_lockfile_patches(lockfile, patched_dependency_hashes.as_ref())
        .map_err(FreshnessCheckError::Stale)
}

/// Parse `pnpm.overrides` from the config. Values can use the
/// `catalog:` protocol, which pnpm resolves against the workspace's
/// catalogs *before* writing them to `pnpm-lock.yaml#overrides` —
/// resolving here keeps an override declared as `"foo": "catalog:"`
/// comparable to the lockfile's already-resolved `"foo": "<concrete>"`.
pub(crate) fn parse_config_overrides(
    config: &Config,
    catalogs: &Catalogs,
) -> Result<Option<Vec<VersionOverride>>, FreshnessCheckError> {
    parse_overrides(config.overrides.as_ref(), catalogs)
}

/// Parses an `overrides` map, from the config or the one a lockfile
/// records. `None` when there are none.
fn parse_overrides(
    overrides: Option<&indexmap::IndexMap<String, String>>,
    catalogs: &Catalogs,
) -> Result<Option<Vec<VersionOverride>>, FreshnessCheckError> {
    match overrides {
        Some(map) if !map.is_empty() => Ok(Some(
            pnpm_config_parse_overrides::parse_overrides_iter(map.iter(), catalogs)
                .map_err(FreshnessCheckError::InvalidOverrides)?,
        )),
        _ => Ok(None),
    }
}

/// Outdated-settings gate: check
/// `ignoredOptionalDependencies` + `overrides` +
/// `packageExtensionsChecksum` drift between the lockfile-recorded
/// values and the current config before the per-importer specifier
/// check.
///
/// `pnpmfile_checksum` is the one input the config doesn't carry.
/// callers that can't produce it pass
/// [`PnpmfileChecksumCheck::Skip`].
#[derive(Clone, Copy)]
pub(crate) struct CheckLockfileSettingsDriftOptions<'a> {
    pub parsed_overrides: Option<&'a [VersionOverride]>,
    pub pnpmfile_checksum: PnpmfileChecksumCheck<'a>,
    pub dedupe_peers: bool,
}

pub(crate) fn check_lockfile_settings_drift(
    lockfile: &Lockfile,
    config: &Config,
    catalogs: &Catalogs,
    opts: CheckLockfileSettingsDriftOptions<'_>,
) -> Result<(), FreshnessCheckError> {
    let CheckLockfileSettingsDriftOptions {
        parsed_overrides,
        pnpmfile_checksum,
        dedupe_peers,
    } = opts;
    let overrides_map: Option<std::collections::HashMap<String, String>> =
        parsed_overrides.map(pnpm_config_parse_overrides::create_overrides_map_from_parsed);
    let package_extensions_checksum =
        crate::install_with_fresh_lockfile::compute_package_extensions_checksum(config);
    // `calcPatchHashes(opts.patchedDependencies)` — reading the patch
    // files here lets `check_lockfile_settings` catch an edited patch
    // whose hash (and thus its `(patch_hash=...)` depPath suffix) drifted
    // from what the lockfile recorded.
    let patched_dependency_hashes =
        config.patched_dependency_hashes().map_err(FreshnessCheckError::CalcPatchHashes)?;
    let resolution_settings = config.lockfile_include_resolution_settings.then(|| {
        crate::fast_update_settings::resolution_settings_from_config(config)
    });
    pnpm_lockfile::check_lockfile_settings(
        lockfile,
        pnpm_lockfile::LockfileSettingsCheck {
            integrity_target: config.lockfile_integrity_target,
            catalogs,
            overrides: overrides_map.as_ref(),
            package_extensions_checksum: package_extensions_checksum.as_deref(),
            ignored_optional_dependencies: config.ignored_optional_dependencies.as_deref(),
            patched_dependencies: patched_dependency_hashes.as_ref(),
            resolution: pnpm_lockfile::ResolutionSettingsCheck {
                auto_install_peers: config.auto_install_peers,
                dedupe_peers,
                exclude_links_from_lockfile: config.exclude_links_from_lockfile,
                add_missing_peer_types: config.add_missing_peer_types,
                inject_workspace_packages: config.inject_workspace_packages,
                peers_suffix_max_length: config.peers_suffix_max_length,
                pnpmfile_checksum,
                resolution_settings: resolution_settings.as_ref(),
            },
        },
    )
    .map_err(FreshnessCheckError::Stale)
}
