use rayon::prelude::*;

use super::{
    Arc, Catalogs, Config, DependencyGroup, Diagnostic, Display, Error, InstallError,
    InstallWithFreshLockfileError, Lockfile, PackageManifest, Path, PathBuf, PnpmfileChecksumCheck,
    StalenessReason, build_project_manifests_list, configured_or_discovered_workspace_dir,
    satisfies_package_manifest,
};

/// Inputs for [`wanted_lockfile_satisfies_workspace`].
pub struct WantedLockfileSatisfactionCheck<'a> {
    pub config: &'a Config,
    /// The active project's manifest; the workspace and its sibling
    /// projects are rediscovered from it exactly the way
    /// [`super::Install::run`] does, so the two agree on the importer
    /// set they compare against the lockfile.
    pub manifest: &'a PackageManifest,
    pub catalogs: &'a Catalogs,
    pub lockfile: &'a Lockfile,
    pub ignore_manifest_check: bool,
}

/// Whether an unfiltered install could materialize `node_modules` from
/// `lockfile` as-is — the same settings-drift and per-importer
/// specifier gates the explicit `--frozen-lockfile` dispatch runs, so a
/// `true` verdict guarantees a subsequent frozen [`super::Install`] run
/// over the same lockfile passes its freshness check instead of
/// erroring.
///
/// Built for the pnpr client, which uses the verdict to skip the
/// server resolve exchange when there is nothing to resolve
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
/// Conservative on every input it cannot cheaply reason about — an
/// empty lockfile, config dependencies, a workspace pnpmfile (whose
/// hooks could rewrite manifests or force a re-resolve), or a failed
/// workspace discovery all report `false`, sending the caller down its
/// full (resolving) path.
pub async fn wanted_lockfile_satisfies_workspace(
    check: &WantedLockfileSatisfactionCheck<'_>,
) -> bool {
    let WantedLockfileSatisfactionCheck {
        config,
        manifest,
        catalogs,
        lockfile,
        ignore_manifest_check,
    } = *check;
    if lockfile.is_empty() {
        return false;
    }
    if config.config_dependencies.as_ref().is_some_and(|deps| !deps.is_empty()) {
        return false;
    }
    let Some(manifest_dir) = manifest.path().parent() else {
        return false;
    };
    let Ok(workspace_dir_opt) = configured_or_discovered_workspace_dir(config, manifest_dir) else {
        return false;
    };
    let workspace_root = workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf());
    // The importer ids below name projects relative to the directory the
    // lockfile sits in, which `lockfileDir` can move away from the
    // workspace root — deriving them from the workspace instead would
    // classify every importer the lockfile records as missing.
    let lockfile_root =
        super::lockfile_root_for(config, workspace_dir_opt.as_deref(), manifest_dir);
    if !config.ignore_pnpmfile
        && !pnpm_hooks::finder::find_pnpmfiles(&workspace_root, crate::pnpmfile_selection(config))
            .is_empty()
    {
        return false;
    }
    let Ok(workspace_manifest) = pnpm_workspace::read_workspace_manifest(&workspace_root) else {
        return false;
    };
    let Ok(workspace_projects) =
        super::load_workspace_projects(&workspace_root, workspace_manifest.as_ref())
    else {
        return false;
    };
    let project_manifests = build_project_manifests_list(manifest, workspace_projects.as_deref());
    let manifest_freshness_inputs: Vec<(String, &PackageManifest)> = project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            (pnpm_workspace::importer_id_from_root_dir(&lockfile_root, project_dir), *manifest)
        })
        .collect();
    check_lockfile_freshness(
        lockfile,
        &manifest_freshness_inputs,
        config,
        catalogs,
        None,
        FreshnessScope {
            ignore_manifest_check,
            // Both stricter than the auto-frozen dispatch: the verdict
            // must imply the explicit-frozen gates pass, and a stale
            // importer needs the resolving path to prune it.
            allow_missing_dependency_free_importers: false,
            prune_stale_importers: true,
        },
    )
    .await
    .is_ok()
}

pub(super) struct FastUpdateLockfileOptions<'a, 'manifest> {
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) manifests: &'a [(String, &'manifest PackageManifest)],
    pub(super) project_manifests: &'a [(PathBuf, &'manifest PackageManifest)],
    pub(super) config: &'a Config,
    pub(super) catalogs: &'a Catalogs,
    pub(super) pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) ignore_manifest_check: bool,
    /// Whether this run sees the complete project list, so an importer
    /// no project claims may be dropped rather than kept.
    pub(super) prune_stale_importers: bool,
}

/// Rewrite the loaded lockfile in place of a full resolution for the
/// drift the lockfile itself proves is safe to absorb — see
/// [`crate::fast_update_compose::try_compose_fast_updates`] for the
/// handlers and their composition order. The candidate only replaces
/// the loaded lockfile once it passes every freshness gate, so a
/// handler that rewrites too much falls back to the resolver instead
/// of committing.
pub(super) async fn try_fast_update_lockfile<Reporter: pnpm_reporter::Reporter>(
    opts: FastUpdateLockfileOptions<'_, '_>,
) -> Option<Lockfile> {
    let lockfile = opts.lockfile?;
    // Hashed once for the whole attempt: the drift check, the unused-patch
    // guard inside the pipeline, and the report below all need the same
    // snapshot, and reading the patch files is the only I/O any of them do.
    // `Err` is a patch file that cannot be read or hashed, which the
    // resolver reports — not the same as having none configured.
    let Ok(patch_hashes) = opts.config.patched_dependency_hashes() else {
        return None;
    };
    let candidate = crate::fast_update_compose::try_compose_fast_updates(
        lockfile,
        opts.manifests,
        opts.project_manifests,
        opts.config,
        patch_hashes.as_ref(),
        opts.prune_stale_importers,
    )?;
    check_lockfile_freshness(
        &candidate,
        opts.manifests,
        opts.config,
        opts.catalogs,
        opts.pnpmfile_hook,
        FreshnessScope {
            ignore_manifest_check: opts.ignore_manifest_check,
            allow_missing_dependency_free_importers: true,
            prune_stale_importers: opts.prune_stale_importers,
        },
    )
    .await
    .ok()?;
    // Only the committed candidate is worth reporting on: a rewrite the
    // freshness gates reject is followed by the resolution, which reports it
    // itself.
    if let Some(unused) =
        crate::fast_update_patched_dependencies::unused_patches(&candidate, patch_hashes.as_ref())
    {
        Reporter::emit(&pnpm_reporter::LogEvent::Global(pnpm_reporter::GlobalLog {
            level: pnpm_reporter::LogLevel::Warn,
            message: unused.to_string(),
        }));
    }
    Some(candidate)
}

/// Which importers a freshness check may reason about, and how
/// strictly. See [`check_lockfile_freshness`] for what each one admits.
#[derive(Clone, Copy)]
pub(crate) struct FreshnessScope {
    /// Skip the per-importer specifier gate entirely.
    pub(crate) ignore_manifest_check: bool,
    /// Treat a project with no importer entry and no dependencies as
    /// satisfied rather than missing.
    pub(crate) allow_missing_dependency_free_importers: bool,
    /// Treat an importer no project claims as staleness. Only an
    /// unfiltered workspace install may, since only it sees the
    /// complete project list.
    pub(crate) prune_stale_importers: bool,
}

/// The first importer the lockfile records that no project claims.
pub(super) fn removed_importer_id<'a>(
    lockfile: &'a Lockfile,
    manifest_freshness_inputs: &[(String, &PackageManifest)],
) -> Option<&'a str> {
    let manifest_ids: std::collections::HashSet<&str> =
        manifest_freshness_inputs.iter().map(|(id, _)| id.as_str()).collect();
    lockfile
        .importers
        .keys()
        .find(|importer_id| !manifest_ids.contains(importer_id.as_str()))
        .map(String::as_str)
}

/// Run every gate the frozen-lockfile dispatch consults before
/// committing to materializing `node_modules` from `lockfile`:
/// `pnpm.overrides` parsing, the settings-drift check
/// ([`pnpm_lockfile::check_lockfile_settings`]), and the
/// per-importer manifest specifier check
/// ([`pnpm_lockfile::satisfies_package_manifest`]).
///
/// Shared between dispatch states 1 and 2 so the explicit
/// `--frozen-lockfile` flag and the implicit `preferFrozenLockfile:
/// true` fast path agree on what "lockfile is up to date" means.
/// Callers in state 1 surface any `Err` as [`InstallError`]; callers
/// in state 2 treat a stale-lockfile `Err` as fall-through to the
/// fresh-resolve path (and surface the rest as fatal — see the
/// `From<FreshnessCheckError> for InstallError` impl below).
///
/// `ignore_manifest_check` skips the per-importer specifier gate.
/// The pnpm CLI passes it when delegating materialization through
/// `configDependencies`: pnpm has just resolved the tree and written
/// the lockfile, but hasn't yet written the post-mutation
/// `package.json` to disk, so the freshness check would always fire
/// on `pnpm up` / `add` / `remove`. Settings drift (`overrides`,
/// `ignoredOptionalDependencies`) still runs.
///
/// `pnpmfile_hook` is the pnpmfile an install of this project would
/// load, whose checksum the settings gate compares against
/// `lockfile.pnpmfileChecksum` (see
/// [`pnpm_hooks::current_pnpmfile_checksum`]).
pub(super) async fn check_lockfile_freshness(
    lockfile: &Lockfile,
    manifest_freshness_inputs: &[(String, &PackageManifest)],
    config: &Config,
    catalogs: &Catalogs,
    pnpmfile_hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    scope: FreshnessScope,
) -> Result<(), FreshnessCheckError> {
    let FreshnessScope {
        ignore_manifest_check,
        allow_missing_dependency_free_importers,
        prune_stale_importers,
    } = scope;
    let parsed_overrides_opt = parse_config_overrides(config, catalogs)?;
    let pnpmfile_checksum =
        pnpm_hooks::current_pnpmfile_checksum(pnpmfile_hook, lockfile.pnpmfile_checksum.as_deref())
            .await;
    check_lockfile_settings_drift(
        lockfile,
        config,
        catalogs,
        CheckLockfileSettingsDriftOptions {
            parsed_overrides: parsed_overrides_opt.as_deref(),
            pnpmfile_checksum: PnpmfileChecksumCheck::Current(pnpmfile_checksum.as_deref()),
            dedupe_peers: config.dedupe_peers,
        },
    )?;

    if ignore_manifest_check {
        return Ok(());
    }

    // An importer whose project is gone leaves the recorded graph wider
    // than the workspace, and it is a root in every reachability walk, so
    // it also keeps that project's dependencies alive. Only an unfiltered
    // install sees the whole project list, so only it may conclude this.
    if prune_stale_importers
        && let Some(importer_id) = removed_importer_id(lockfile, manifest_freshness_inputs)
    {
        return Err(FreshnessCheckError::Stale(StalenessReason::RemovedImporter {
            importer_id: importer_id.to_string(),
        }));
    }

    let ignored_optional_matcher = pnpm_config::matcher::create_matcher(
        config.ignored_optional_dependencies.as_deref().unwrap_or_default(),
    );
    // Each importer's check reads only shared references, so a
    // workspace-scale importer list fans out across the rayon pool; the
    // serial fold keeps the first error in importer order, like the
    // loop it replaces.
    let results: Vec<Result<(), FreshnessCheckError>> = manifest_freshness_inputs
        .par_iter()
        .map(|(importer_id, manifest)| {
            if allow_missing_dependency_free_importers
                && !lockfile.importers.contains_key(importer_id)
                && !manifest_has_effective_dependencies(manifest, &ignored_optional_matcher)
            {
                return Ok(());
            }
            check_importer_satisfies(
                lockfile,
                manifest,
                importer_id,
                config,
                &ignored_optional_matcher,
                parsed_overrides_opt.as_deref(),
            )
        })
        .collect();
    for result in results {
        result?;
    }
    Ok(())
}

/// Parse `pnpm.overrides` from the config. Values can use the
/// `catalog:` protocol, which pnpm resolves against the workspace's
/// catalogs *before* writing them to `pnpm-lock.yaml#overrides` —
/// resolving here keeps an override declared as `"foo": "catalog:"`
/// comparable to the lockfile's already-resolved `"foo": "<concrete>"`.
pub(crate) fn parse_config_overrides(
    config: &Config,
    catalogs: &Catalogs,
) -> Result<Option<Vec<pnpm_config_parse_overrides::VersionOverride>>, FreshnessCheckError> {
    match config.overrides.as_ref() {
        Some(map) if !map.is_empty() => Ok(Some(
            pnpm_config_parse_overrides::parse_overrides_iter(map.iter(), catalogs)
                .map_err(FreshnessCheckError::InvalidOverrides)?,
        )),
        _ => Ok(None),
    }
}

/// Outdated-settings gate (umbrella <https://github.com/pnpm/pacquet/issues/434> slice 7): check
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
    pub parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    pub pnpmfile_checksum: PnpmfileChecksumCheck<'a>,
    pub dedupe_peers: bool,
}

pub(crate) fn check_lockfile_settings_drift(
    lockfile: &Lockfile,
    config: &Config,
    catalogs: &Catalogs,
    opts: CheckLockfileSettingsDriftOptions<'_>,
) -> Result<(), FreshnessCheckError> {
    let CheckLockfileSettingsDriftOptions { parsed_overrides, pnpmfile_checksum, dedupe_peers } =
        opts;
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
    pnpm_lockfile::check_lockfile_settings(
        lockfile,
        pnpm_lockfile::LockfileSettingsCheck {
            catalogs,
            overrides: overrides_map.as_ref(),
            package_extensions_checksum: package_extensions_checksum.as_deref(),
            ignored_optional_dependencies: config.ignored_optional_dependencies.as_deref(),
            patched_dependencies: patched_dependency_hashes.as_ref(),
            auto_install_peers: config.auto_install_peers,
            dedupe_peers,
            exclude_links_from_lockfile: config.exclude_links_from_lockfile,
            inject_workspace_packages: config.inject_workspace_packages,
            peers_suffix_max_length: config.peers_suffix_max_length,
            pnpmfile_checksum,
        },
    )
    .map_err(FreshnessCheckError::Stale)
}

/// Per-importer slice of the freshness gate: the manifest of the
/// project at `importer_id` must still be satisfied by the lockfile's
/// importer snapshot.
pub(crate) fn check_importer_satisfies(
    lockfile: &Lockfile,
    manifest: &PackageManifest,
    importer_id: &str,
    config: &Config,
    ignored_optional_matcher: &pnpm_config::matcher::Matcher,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
) -> Result<(), FreshnessCheckError> {
    let importer = lockfile
        .importers
        .get(importer_id)
        .ok_or_else(|| FreshnessCheckError::NoImporter { importer_id: importer_id.to_string() })?;

    // Apply `pnpm.overrides` to a *cloned* manifest before the
    // per-importer specifier check so the lockfile's specifiers —
    // written with overrides already applied — match the on-disk
    // manifest's deps. The caller's manifest stays pristine since the
    // override pass conceptually returns a new manifest
    // from the perspective of every consumer downstream of the
    // resolver.
    // `auto_install_peers` is folded into `satisfies_package_manifest`
    // itself, so the manifest is cloned here only for the two mutations the
    // comparison needs done up front: applying `pnpm.overrides` and dropping
    // `link:` deps under `exclude_links_from_lockfile`.
    let normalized_manifest_holder;
    let manifest_for_freshness: &PackageManifest = if parsed_overrides.is_some()
        || config.exclude_links_from_lockfile
    {
        let root_dir = manifest.path().parent().unwrap_or_else(|| Path::new("."));
        normalized_manifest_holder = {
            let mut cloned: PackageManifest = manifest.clone();
            if let Some(parsed) = parsed_overrides {
                crate::VersionsOverrider::new(parsed, root_dir).apply(&mut cloned, Some(root_dir));
            }
            if config.exclude_links_from_lockfile {
                exclude_linked_dependencies(&mut cloned);
            }
            cloned
        };
        &normalized_manifest_holder
    } else {
        manifest
    };

    // Build the `ignoredOptionalDependencies` filter set: iterate
    // `manifest.optionalDependencies` and delete matches from BOTH the
    // `optional` and `dependencies` maps. A name only present in
    // `dependencies` that happens to match the
    // pattern is NOT removed — set-based ("name was in
    // optionalDependencies AND matched") rather than pure pattern
    // matching. `devDependencies` is untouched on purpose; the group
    // gate inside `satisfies_package_manifest` enforces that.
    let ignored_set =
        ignored_optional_dependency_names(manifest_for_freshness, ignored_optional_matcher);
    let is_ignored_optional: &dyn Fn(&str) -> bool = &|name: &str| ignored_set.contains(name);

    satisfies_package_manifest(
        importer,
        manifest_for_freshness,
        config.auto_install_peers,
        is_ignored_optional,
    )
    .map_err(|reason| {
        // Stamp the importer onto a specifier diff so the workspace-wide
        // freshness report names the drifted project, not only the dep.
        let reason = match reason {
            StalenessReason::SpecifiersDiffer(mut diff) => {
                diff.importer_id = Some(importer_id.to_string());
                StalenessReason::SpecifiersDiffer(diff)
            }
            other => other,
        };
        FreshnessCheckError::Stale(reason)
    })
}

pub(super) fn ignored_optional_dependency_names(
    manifest: &PackageManifest,
    matcher: &pnpm_config::matcher::Matcher,
) -> std::collections::HashSet<String> {
    manifest
        .dependencies([pnpm_package_manifest::DependencyGroup::Optional])
        .filter(|(name, _)| matcher.matches(name))
        .map(|(name, _)| name.to_string())
        .collect()
}

pub(super) fn manifest_has_effective_dependencies(
    manifest: &PackageManifest,
    ignored_optional_matcher: &pnpm_config::matcher::Matcher,
) -> bool {
    if manifest.dependencies([pnpm_package_manifest::DependencyGroup::Dev]).next().is_some() {
        return true;
    }
    let ignored = ignored_optional_dependency_names(manifest, ignored_optional_matcher);
    manifest
        .dependencies([
            pnpm_package_manifest::DependencyGroup::Prod,
            pnpm_package_manifest::DependencyGroup::Optional,
        ])
        .any(|(name, _)| !ignored.contains(name))
}

pub(super) fn exclude_linked_dependencies(manifest: &mut PackageManifest) {
    let Some(manifest) = manifest.value_mut().as_object_mut() else {
        return;
    };
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        let group: &str = group.into();
        if let Some(dependencies) =
            manifest.get_mut(group).and_then(serde_json::Value::as_object_mut)
        {
            dependencies.retain(|_, specifier| {
                let Some(specifier) = specifier.as_str() else {
                    return true;
                };
                !specifier.starts_with("link:")
            });
        }
    }
}

/// Outcome of [`check_lockfile_freshness`]. Splits "user
/// configuration is malformed" (always fatal) from "lockfile is stale"
/// (fatal for `--frozen-lockfile`, fall-through to the fresh-resolve
/// path under `preferFrozenLockfile: true`).
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum FreshnessCheckError {
    /// The lockfile has no entry for the root importer.
    #[display(
        r#"Cannot install with "frozen-lockfile" because pnpm-lock.yaml has no `importers["{importer_id}"]` entry. Regenerate the lockfile with `pnpm install --lockfile-only`."#
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER))]
    NoImporter { importer_id: String },

    /// A value in `pnpm.overrides` couldn't be parsed.
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pnpm_config_parse_overrides::ParseOverridesError),

    /// A configured `patchedDependencies` patch file couldn't be read
    /// or hashed while computing the map to compare against the
    /// lockfile.
    #[diagnostic(transparent)]
    CalcPatchHashes(#[error(source)] pnpm_patching::CalcPatchHashError),

    /// `pnpm-lock.yaml` doesn't match the on-disk `package.json` /
    /// current settings.
    #[display("{_0}")]
    Stale(#[error(not(source))] StalenessReason),
}

impl From<FreshnessCheckError> for InstallError {
    fn from(error: FreshnessCheckError) -> InstallError {
        match error {
            FreshnessCheckError::NoImporter { importer_id } => {
                InstallError::NoImporter { importer_id }
            }
            FreshnessCheckError::InvalidOverrides(inner) => InstallError::InvalidOverrides(inner),
            FreshnessCheckError::CalcPatchHashes(inner) => InstallError::WithFreshLockfile(
                InstallWithFreshLockfileError::CalcPatchHashes(inner),
            ),
            FreshnessCheckError::Stale(reason) => match reason.setting_name() {
                Some(setting) => InstallError::LockfileConfigMismatch { setting },
                None => InstallError::OutdatedLockfile { reason },
            },
        }
    }
}
