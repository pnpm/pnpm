//! Pre-install fast path: when nothing has changed since the last
//! install, skip the entire pipeline.
//!
//! The install logs "Already up to date" when nothing has changed,
//! before any of the install setup runs. The check keys off
//! `<workspace_root>/node_modules/.pnpm-workspace-state-v1.json`'s
//! `lastValidatedTimestamp` against each project's `package.json`
//! mtime — never touching the lockfile, the verifier cache, or any
//! resolver state.
//!
//! Scope: the mtime-vs-`lastValidatedTimestamp` branch (the
//! up-to-date exit when no project is modified), the patch-file branch
//! (a configured patch file whose mtime is newer than
//! `lastValidatedTimestamp` invalidates the fast path even when its
//! `patchedDependencies` config entry is unchanged — a content edit the
//! key→path settings comparison can't see), and the modified-manifests
//! content re-check: when a manifest's mtime is newer but its
//! dependency-relevant content still matches the lockfile, the install
//! still reports up-to-date (a `touch package.json`, a `scripts` edit, or
//! an `npm pkg set/delete` rewrite must not trigger a full install), and
//! the pnpmfile branch (an added, removed, or edited workspace pnpmfile
//! invalidates the fast path; plugin pnpmfiles from config dependencies
//! are covered by the `config_dependencies` comparison instead of the
//! mtime check), and the local-file-dependency bail: no tracked mtime
//! covers the *contents* of a local file dependency (a `file:` specifier
//! or a bare local path/tarball spec, declared directly or through a
//! `pnpm.overrides` entry), so projects declaring one always take the
//! full install path, which refetches those dependencies. The
//! local-file-dependency freshness branch of linked-package verification
//! is NOT ported here. When this function returns `Decision::Skipped` the
//! caller proceeds with the full install path, which still has its own
//! freshness guards (`check_lockfile_freshness`, the no-op
//! short-circuit).
//!
//! ## Why a separate module
//!
//! Lives in `pacquet-package-manager` rather than a new
//! `pacquet-deps-status` crate because both consumers — `Install::run`
//! and the verify-deps-before-run gate ([`check_deps_status_before_run`])
//! — lean on install internals (`check_lockfile_settings_drift`,
//! `check_importer_satisfies`, `build_workspace_state`) that a separate
//! crate would have to re-export wholesale. Extract it only if a
//! consumer outside this crate's dependents appears.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use pacquet_catalogs_resolver::{CatalogResolutionResult, WantedDependency, resolve_from_catalog};
use pacquet_catalogs_types::Catalogs;
use pacquet_config::{Config, LinkWorkspacePackages, NodeLinker};
use pacquet_lockfile::{ImporterDepVersion, Lockfile, MaybeLazyLockfile, ProjectSnapshot};
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_workspace_state::{
    NodeLinker as WorkspaceStateNodeLinker, WorkspaceState, WorkspaceStateSettings,
    load_workspace_state, update_workspace_state,
};

/// Outcome of [`check_optimistic_repeat_install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The install is fully up to date — emit "Already up to date"
    /// and exit before any of the install setup runs.
    UpToDate,
    /// Fall through to the full install path. `reason` is a short
    /// diagnostic string surfaced via `tracing::debug!` for
    /// diagnosability without contaminating the reporter stream.
    Skipped { reason: &'static str },
}

/// Inputs to [`check_optimistic_repeat_install`].
pub struct OptimisticRepeatInstallCheck<'a> {
    /// The directory containing `pnpm-workspace.yaml` (or the project
    /// root when no workspace manifest exists — same fallback as
    /// [`Install::run`](crate::Install::run)).
    pub workspace_root: &'a Path,
    pub config: &'a Config,
    pub node_linker: NodeLinker,
    pub included: IncludedDependencies,
    /// Every importer's `(root_dir, manifest)` pair. For a
    /// single-project install it's just the root manifest; for a
    /// workspace install it's every project the resolver would
    /// otherwise walk. The caller passes this in (rather than this
    /// function rediscovering it) so the same walk seeds the regular
    /// install path on the fall-through.
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    /// `true` when a `pnpm-workspace.yaml` drives the install — that
    /// selects the workspace branch, which keys the manifest and
    /// lockfile comparisons off `lastValidatedTimestamp`. `false` (no
    /// workspace manifest) selects the single-project branch, which
    /// additionally requires `pnpm-lock.yaml` to exist on disk —
    /// `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` is raised otherwise, which
    /// resolves to not-up-to-date — and keys its comparisons off the
    /// lockfile mtimes instead.
    pub is_workspace_install: bool,
    /// The wanted lockfile (`None` once loaded when `pnpm-lock.yaml`
    /// is absent or empty). Consulted only by the modified-manifests
    /// content re-check; the pure-mtime fast path never reads it —
    /// which is why it arrives lazily, so the common repeat-install
    /// run skips the YAML parse entirely. When absent and
    /// `<virtual_store_dir>/lock.yaml` exists, the current lockfile
    /// stands in as the wanted one — it records exactly what the
    /// previous install materialized — and `pnpm-lock.yaml` is
    /// regenerated from it before the check reports up-to-date.
    pub lockfile: MaybeLazyLockfile<'a>,
    /// Catalogs from the workspace manifest or an `updateConfig`
    /// pnpmfile hook, for resolving `catalog:` values inside
    /// `pnpm.overrides` before the lockfile settings comparison.
    pub catalogs: &'a Catalogs,
}

/// Run the workspace-state freshness fast path. Returns
/// [`Decision::UpToDate`] when the install can short-circuit.
///
/// Always returns `Decision::Skipped` when
/// `config.optimistic_repeat_install` is `false`.
pub fn check_optimistic_repeat_install(check: &OptimisticRepeatInstallCheck<'_>) -> Decision {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included,
        project_manifests,
        is_workspace_install,
        catalogs,
        ..
    } = check;
    if !config.optimistic_repeat_install {
        return Decision::Skipped { reason: "optimistic_repeat_install disabled" };
    }

    // No workspace state means no previous install has completed
    // (or the file was deleted) — there's no `lastValidatedTimestamp`
    // to compare against.
    let Ok(Some(state)) = load_workspace_state(workspace_root) else {
        return Decision::Skipped { reason: "no workspace state on disk" };
    };

    // Unconditional here because the only caller is the install
    // command, which always treats local file deps as outdated.
    if has_local_file_dep(project_manifests, included, catalogs) {
        return Decision::Skipped {
            reason: "a dependency is a local file dependency and its contents may have changed",
        };
    }
    match has_local_file_override(config, catalogs) {
        Ok(true) => {
            return Decision::Skipped {
                reason: "an override maps to a local file dependency and its contents may have changed",
            };
        }
        Err(reason) => return Decision::Skipped { reason },
        Ok(false) => {}
    }
    if has_local_file_package_extension(config, included, catalogs) {
        return Decision::Skipped {
            reason: "a package extension injects a local file dependency and its contents may have changed",
        };
    }

    if !settings_match(&state, config, node_linker, included) {
        return Decision::Skipped { reason: "settings drift" };
    }

    if !catalogs_cache_matches(state.settings.catalogs.as_ref(), catalogs) {
        return Decision::Skipped { reason: "catalogs cache outdated" };
    }

    if !project_structure_matches(&state, project_manifests) {
        return Decision::Skipped { reason: "workspace project list changed" };
    }

    // The "modules dir exists when the project has deps" gate: a
    // project with `dependencies`/`devDependencies` but no
    // `node_modules` cannot be up to date. The `modulesDir` is read
    // off the per-project config; pacquet doesn't track per-importer
    // overrides yet, so check the install-time `config.modules_dir`
    // for the root + `<project_root>/node_modules` for siblings,
    // matching the `isolated`-linker default.
    if !modules_dirs_present(config, project_manifests) {
        return Decision::Skipped {
            reason: "project has dependencies but no node_modules directory",
        };
    }

    // Single-project installs require a lockfile to even attempt the
    // fast path. The single-project branch raises
    // `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` when the wanted-lockfile
    // stat is absent, which resolves to not-up-to-date. Pacquet
    // additionally accepts the *current* lockfile
    // (`<virtual_store_dir>/lock.yaml`) as a stand-in when
    // `pnpm-lock.yaml` is missing: it records exactly what the
    // previous install materialized, so the content checks can run
    // against it and `pnpm-lock.yaml` is regenerated from it on
    // success — the same substitution the full install path makes
    // when it synthesizes the wanted lockfile from the current one.
    // Workspace installs skip this existence gate — the workspace
    // branch tolerates a missing `pnpm-lock.yaml` (the wanted-lockfile
    // scan `continue`s on ENOENT, and the missing lockfile is restored
    // from the current one rather than failing). The mtime side of that
    // probe is handled by `wanted_lockfile_modified` below.
    if !is_workspace_install
        && !workspace_root.join(Lockfile::FILE_NAME).exists()
        && !current_lockfile_file_has_content(&config.virtual_store_dir)
    {
        return Decision::Skipped { reason: "wanted lockfile missing" };
    }

    // A patch file edited in place keeps the same `patchedDependencies`
    // key→path entry (so `settings_match` can't see the change) but
    // changes the patched output and the patch hash. This check runs
    // before the manifest-modified exit so the patch reason wins when
    // both a patch and a manifest are newer than the last validation.
    if patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return Decision::Skipped { reason: "a patch file is newer than the last validation" };
    }

    // A pnpmfile added, removed, or edited in place can change
    // resolution (readPackage rewrites, custom resolvers, a
    // `shouldRefreshResolution` verdict) without touching any manifest,
    // so it must defeat the mtime fast path.
    if pnpmfiles_modified_since(workspace_root, &state.pnpmfiles, state.last_validated_timestamp) {
        return Decision::Skipped { reason: "a pnpmfile changed since the last validation" };
    }

    // The fast-path conclusion: walk every manifest and report up to
    // date when none have an mtime newer than
    // `workspaceState.lastValidatedTimestamp`. The walk has to
    // succeed (read errors mean we can't *prove* freshness, so fall
    // through).
    let Some(manifest_stats) = stat_manifests(project_manifests) else {
        return Decision::Skipped { reason: "failed to stat a project manifest" };
    };
    let modified: Vec<&ManifestStat<'_>> = manifest_stats
        .iter()
        .filter(|stat| stat.mtime_ms > state.last_validated_timestamp)
        .collect();

    // A lockfile-only change — `git checkout`/stash-restore of just
    // `pnpm-lock.yaml`, or an external rewrite — leaves every manifest
    // untouched but still invalidates the install. Probe the wanted
    // lockfile's mtime before the manifest-mtime exit so a lockfile
    // modification is not missed.
    let lockfile_modified =
        wanted_lockfile_modified(workspace_root, state.last_validated_timestamp);

    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => return Decision::Skipped { reason: "current lockfile missing" },
        Ok(false) => {}
        Err(reason) => return Decision::Skipped { reason },
    }

    if modified.is_empty() && !lockfile_modified {
        return match regenerate_wanted_lockfile_if_missing(check, None) {
            Ok(()) => Decision::UpToDate,
            Err(reason) => Decision::Skipped { reason },
        };
    }

    // A newer mtime alone doesn't invalidate: the modified-manifests
    // branch re-checks the *content* against the wanted lockfile so a
    // rewrite that left the dependency fields intact — `touch`, a
    // `scripts` edit, `npm pkg set/delete` — still reports up to date.
    // When only the lockfile changed, every project is validated rather
    // than just the modified ones.
    let projects_to_check: Vec<&ManifestStat<'_>> =
        if lockfile_modified { manifest_stats.iter().collect() } else { modified };
    match modified_manifests_match_lockfile(check, &state, &projects_to_check) {
        Ok(loaded_current) => {
            if let Err(reason) = regenerate_wanted_lockfile_if_missing(check, loaded_current) {
                return Decision::Skipped { reason };
            }
            // Update `lastValidatedTimestamp` to prevent a pointless
            // repeat: the workspace branch rewrites the state after the
            // content checks pass. The single-project branch keys its
            // comparisons off the lockfile mtimes instead and leaves the
            // state alone. A failed write only costs the next run a
            // repeat of the content check, so it degrades rather than
            // fails.
            if is_workspace_install {
                let new_state = crate::install::build_workspace_state(
                    workspace_root,
                    config,
                    node_linker,
                    included,
                    catalogs,
                    project_manifests,
                );
                if let Err(error) = update_workspace_state(workspace_root, &new_state) {
                    tracing::warn!(
                        target: "pacquet::install",
                        ?error,
                        "Failed to refresh the workspace state after the repeat-install content check",
                    );
                }
            }
            Decision::UpToDate
        }
        Err(reason) => Decision::Skipped { reason },
    }
}

/// Whether any project declares a dependency with a local file
/// specifier in `dependencies`, `devDependencies`, or
/// `optionalDependencies`. Groups excluded from the current install
/// (per `included`) are skipped. `catalog:` specs are dereferenced
/// through the workspace catalogs.
fn has_local_file_dep(
    project_manifests: &[(PathBuf, &PackageManifest)],
    included: IncludedDependencies,
    catalogs: &Catalogs,
) -> bool {
    let fields: [(&str, bool); 3] = [
        ("dependencies", included.dependencies),
        ("devDependencies", included.dev_dependencies),
        ("optionalDependencies", included.optional_dependencies),
    ];
    project_manifests.iter().any(|(_, manifest)| {
        fields.iter().any(|(field, group_included)| {
            *group_included
                && manifest.value().get(*field).and_then(|value| value.as_object()).is_some_and(
                    |deps| {
                        deps.iter().any(|(alias, spec)| {
                            spec.as_str().is_some_and(|spec| {
                                is_local_file_spec(spec)
                                    || catalog_resolves_to_local_file(catalogs, alias, spec)
                            })
                        })
                    },
                )
        })
    })
}

/// Whether a `catalog:` spec dereferences (through the workspace
/// catalogs) to a local file specifier. A misconfigured catalog entry
/// returns `false`: it fails the full install with the proper error
/// anyway, so the fast path only needs to not report up-to-date for a
/// *valid* catalog entry holding a local path.
fn catalog_resolves_to_local_file(catalogs: &Catalogs, alias: &str, spec: &str) -> bool {
    // `resolve_from_catalog` returns `Unused` for any non-`catalog:` spec, so
    // short-circuit before allocating the owned `WantedDependency` it needs.
    if !spec.starts_with("catalog:") {
        return false;
    }
    match resolve_from_catalog(
        catalogs,
        &WantedDependency { alias: alias.to_string(), bare_specifier: spec.to_string() },
    ) {
        CatalogResolutionResult::Found(found) => is_local_file_spec(&found.resolution.specifier),
        _ => false,
    }
}

/// Whether any `pnpm.overrides` entry maps to a local file specifier.
/// An override redirects every matching dependency in the graph to its
/// specifier, so a local file override makes the installed contents
/// depend on that directory or tarball the same way a direct local file
/// dependency does. A parse failure returns its own distinct reason —
/// not the local-file reason, which would misattribute the cause.
fn has_local_file_override(config: &Config, catalogs: &Catalogs) -> Result<bool, &'static str> {
    match crate::install::parse_config_overrides(config, catalogs) {
        Ok(Some(overrides)) => {
            Ok(overrides.iter().any(|entry| is_local_file_spec(&entry.new_bare_specifier)))
        }
        Ok(None) => Ok(false),
        Err(_) => Err("pnpm.overrides cannot be parsed"),
    }
}

/// Whether any `packageExtensions` entry injects a dependency with a
/// local file specifier. Package extensions are merged into matching
/// packages' manifests by the read-package hook during the full
/// install, so a `file:`/local-path/tarball spec added there has the
/// same content-change blind spot as a direct local file dependency
/// without appearing in any project manifest. Only `dependencies` and
/// `optionalDependencies` are scanned: peer dependencies are resolved
/// from the graph rather than fetched, so a local spec there is never
/// installed.
fn has_local_file_package_extension(
    config: &Config,
    included: IncludedDependencies,
    catalogs: &Catalogs,
) -> bool {
    let Some(extensions) = config.package_extensions.as_ref() else {
        return false;
    };
    extensions.values().any(|extension| {
        let optional = included
            .optional_dependencies
            .then_some(extension.optional_dependencies.as_ref())
            .flatten();
        [extension.dependencies.as_ref(), optional].into_iter().flatten().any(|deps| {
            deps.iter().any(|(alias, spec)| {
                is_local_file_spec(spec) || catalog_resolves_to_local_file(catalogs, alias, spec)
            })
        })
    })
}

/// Whether the specifier resolves to a local directory or tarball whose
/// contents can change without any manifest or lockfile mtime moving:
/// the `file:` protocol, path-prefixed specs (`./`, `../`, `~/`,
/// absolute POSIX paths, and Windows drive paths including
/// drive-relative ones like `c:dir`), and bare tarball file names.
///
/// Deliberately narrower than the local resolver's bare-path matching:
/// a bare path like `user/repo` is statically indistinguishable from a
/// git shorthand at this layer, and matching it would disable the
/// repeat-install fast path for every project with git dependencies.
/// Such specs (and anything else carrying a protocol or URL) stay on
/// the fast path. `catalog:` specs also return `false` here — callers
/// dereference them through the workspace catalogs first, because a
/// catalog entry may hold a bare local path (the catalog resolver only
/// bans the `workspace:`, `link:`, and `file:` protocols).
fn is_local_file_spec(spec: &str) -> bool {
    if spec.starts_with("file:") {
        return true;
    }
    if spec.starts_with(['.', '/', '\\'])
        || spec.starts_with("~/")
        || spec.starts_with(r"~\")
        || is_windows_drive_path(spec)
    {
        return true;
    }
    if spec.contains(':') {
        return false;
    }
    if spec.contains('#') {
        return false;
    }
    ends_with_ignore_ascii_case(spec, ".tgz")
        || ends_with_ignore_ascii_case(spec, ".tar.gz")
        || ends_with_ignore_ascii_case(spec, ".tar")
}

/// Case-insensitive (ASCII) suffix check that, unlike
/// `spec.to_ascii_lowercase().ends_with(suffix)`, does not allocate.
fn ends_with_ignore_ascii_case(spec: &str, suffix: &str) -> bool {
    let spec = spec.as_bytes();
    let suffix = suffix.as_bytes();
    spec.len() >= suffix.len() && spec[spec.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// `c:/...`, `c:\...`, or drive-relative `c:foo` — a Windows drive
/// path. No separator is required after the colon; no registry protocol
/// is a single letter, so `[a-z]:` is unambiguous.
fn is_windows_drive_path(spec: &str) -> bool {
    let bytes = spec.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Restore a missing `pnpm-lock.yaml` from the current lockfile before
/// the fast path reports "Already up to date", so the short-circuit
/// leaves the same on-disk contract a full install would (the full
/// path synthesizes the wanted lockfile from the current one and
/// rewrites it). No-op when `pnpm-lock.yaml` was loaded, when lockfile
/// writing is disabled (`lockfile: false`), or when there is no
/// current lockfile to restore from (a dependency-less project).
/// A write failure falls through to the full install path rather than
/// reporting up-to-date while leaving the lockfile missing.
fn regenerate_wanted_lockfile_if_missing(
    check: &OptimisticRepeatInstallCheck<'_>,
    loaded_current: Option<Lockfile>,
) -> Result<(), &'static str> {
    if check.lockfile.is_loaded_or_on_disk() || !check.config.lockfile {
        return Ok(());
    }
    let current = match loaded_current {
        Some(current) => Some(current),
        None => Lockfile::load_current_from_virtual_store_dir(&check.config.virtual_store_dir)
            .map_err(|_| "the current lockfile cannot be loaded")?,
    };
    let Some(current) = current else {
        return Ok(());
    };
    current
        .save_to_path(&check.workspace_root.join(Lockfile::FILE_NAME))
        .map_err(|_| "failed to regenerate pnpm-lock.yaml from the current lockfile")
}

/// One project manifest's stat outcome, paired with the inputs the
/// content re-check needs.
struct ManifestStat<'a> {
    root_dir: &'a Path,
    manifest: &'a PackageManifest,
    mtime_ms: i64,
}

/// The modified-manifests branch: the lockfile-equality assertion plus
/// the wanted-lockfile up-to-date check (settings drift, per-importer
/// specifier match, linked-package freshness) for every project whose
/// manifest is newer than the last validation. `Err` carries the
/// `Decision::Skipped` reason.
///
/// When `pnpm-lock.yaml` is absent, the current lockfile stands in as
/// the wanted one (see the lockfile gate in
/// [`check_optimistic_repeat_install`]); `Ok(Some(_))` then carries the
/// loaded current lockfile so the caller can regenerate
/// `pnpm-lock.yaml` from it without a second read.
fn modified_manifests_match_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    modified: &[&ManifestStat<'_>],
) -> Result<Option<Lockfile>, &'static str> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        project_manifests,
        is_workspace_install,
        lockfile,
        catalogs,
        ..
    } = check;
    let mut loaded_current: Option<Lockfile> = None;
    let mut wanted_is_current = false;
    let lockfile = lockfile.get().map_err(|_| "the wanted lockfile cannot be read or parsed")?;
    let (wanted, wanted_mtime_ms): (&Lockfile, i64) = if let Some(wanted) = lockfile {
        let Some(mtime) = mtime_ms(&workspace_root.join(Lockfile::FILE_NAME)) else {
            return Err(
                "a manifest is newer than the last validation and pnpm-lock.yaml cannot be stat'd",
            );
        };
        (wanted, mtime)
    } else {
        let current_path = config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
        let Some(mtime) = mtime_ms(&current_path) else {
            return Err("a manifest is newer than the last validation and no lockfile is loaded");
        };
        let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
            .map_err(|_| "the current lockfile cannot be loaded")?
            .ok_or("a manifest is newer than the last validation and no lockfile is loaded")?;
        wanted_is_current = true;
        (&*loaded_current.insert(current), mtime)
    };

    // Decide which modified projects need the full content check, and
    // whether the wanted lockfile must be compared against the current
    // one (`<virtual_store_dir>/lock.yaml`).
    let to_check: &[&ManifestStat<'_>] = if wanted_is_current {
        // The wanted lockfile IS the current one — there's no second
        // lockfile to assert equality against, and the mtime
        // short-circuits below compare the two lockfile files, so they
        // don't apply. Every modified project gets the content check.
        modified
    } else if is_workspace_install {
        // Workspace branch: a wanted lockfile newer than the last
        // validation must equal what the previous install materialized.
        if wanted_mtime_ms > state.last_validated_timestamp {
            assert_wanted_lockfile_equals_current(wanted, config)?;
        }
        modified
    } else {
        // Single-project branch keys off the lockfile mtimes instead of
        // `lastValidatedTimestamp`.
        let current_mtime_ms =
            mtime_ms(&config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME));
        if let Some(current_mtime_ms) = current_mtime_ms
            && wanted_mtime_ms > current_mtime_ms
        {
            assert_wanted_lockfile_equals_current(wanted, config)?;
        }
        let root = modified.first().expect("modified-manifests branch requires a modified project");
        if root.mtime_ms > wanted_mtime_ms {
            modified
        } else if current_mtime_ms.is_some() {
            // "The manifest file is not newer than the lockfile.
            // Exiting check."
            &[]
        } else if !wanted.is_empty() {
            // RUN_CHECK_DEPS_NO_DEPS: the lockfile requires
            // dependencies but nothing was ever installed.
            return Err("the lockfile requires dependencies but none were installed");
        } else {
            &[]
        }
    };

    if to_check.is_empty() {
        return Ok(loaded_current);
    }

    let parsed_overrides = crate::install::parse_config_overrides(config, catalogs)
        .map_err(|_| "pnpm.overrides cannot be parsed")?;
    if let Err(error) = crate::install::check_lockfile_settings_drift(
        wanted,
        config,
        catalogs,
        parsed_overrides.as_deref(),
    ) {
        tracing::debug!(target: "pacquet::install", %error, "repeat-install content check: lockfile settings drift");
        return Err("a lockfile setting drifted from the current configuration");
    }

    let linked_ctx = LinkedPackagesContext::new(config, project_manifests);
    for project in to_check {
        let importer_id =
            pacquet_workspace::importer_id_from_root_dir(workspace_root, project.root_dir);
        if let Err(error) = crate::install::check_importer_satisfies(
            wanted,
            project.manifest,
            &importer_id,
            config,
            parsed_overrides.as_deref(),
        ) {
            tracing::debug!(target: "pacquet::install", %error, importer_id, "repeat-install content check: manifest no longer satisfied");
            return Err("a modified manifest is no longer satisfied by the lockfile");
        }
        let Some(importer) = wanted.importers.get(&importer_id) else {
            return Err("a modified project has no importer entry in the lockfile");
        };
        if !linked_packages_are_up_to_date(
            &linked_ctx,
            project.root_dir,
            project.manifest,
            importer,
        ) {
            return Err("a linked package is out of date");
        }
    }
    Ok(loaded_current)
}

/// Assert the wanted lockfile equals the current one: with no current
/// lockfile every importer of the wanted one must be dependency-free
/// (`RUN_CHECK_DEPS_NO_DEPS`); otherwise the two parsed lockfiles must
/// be equal (`RUN_CHECK_DEPS_OUTDATED_DEPS`).
fn assert_wanted_lockfile_equals_current(
    wanted: &Lockfile,
    config: &Config,
) -> Result<(), &'static str> {
    let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
        .map_err(|_| "the current lockfile cannot be loaded")?;
    match current {
        None => {
            let any_deps = wanted.importers.values().any(|snapshot| {
                snapshot
                    .dependencies_by_groups([
                        DependencyGroup::Prod,
                        DependencyGroup::Dev,
                        DependencyGroup::Optional,
                    ])
                    .next()
                    .is_some()
            });
            if any_deps {
                Err("the lockfile requires dependencies but none were installed")
            } else {
                Ok(())
            }
        }
        Some(current) => {
            if &current == wanted {
                Ok(())
            } else {
                Err("the installed dependencies are not up to date with the lockfile")
            }
        }
    }
}

/// Shared lookups for [`linked_packages_are_up_to_date`], built once
/// per content check.
struct LinkedPackagesContext<'a> {
    link_workspace_packages: bool,
    manifests_by_dir: std::collections::HashMap<&'a Path, &'a PackageManifest>,
    /// `name → version → root_dir` over the workspace's projects.
    workspace_packages:
        std::collections::HashMap<String, std::collections::HashMap<String, &'a Path>>,
}

impl<'a> LinkedPackagesContext<'a> {
    fn new(config: &Config, project_manifests: &'a [(PathBuf, &'a PackageManifest)]) -> Self {
        let mut manifests_by_dir = std::collections::HashMap::new();
        let mut workspace_packages: std::collections::HashMap<
            String,
            std::collections::HashMap<String, &'a Path>,
        > = std::collections::HashMap::new();
        for (root_dir, manifest) in project_manifests {
            manifests_by_dir.insert(root_dir.as_path(), *manifest);
            if let (Some(name), Some(version)) = (
                manifest_string_field(manifest, "name"),
                manifest_string_field(manifest, "version"),
            ) {
                workspace_packages.entry(name).or_default().insert(version, root_dir.as_path());
            }
        }
        LinkedPackagesContext {
            link_workspace_packages: config.link_workspace_packages != LinkWorkspacePackages::Off,
            manifests_by_dir,
            workspace_packages,
        }
    }

    /// The version of the package manifest at `dir`, preferring the
    /// already-loaded workspace manifests over a disk read.
    fn linked_version(&self, dir: &Path) -> Option<String> {
        if let Some(manifest) = self.manifests_by_dir.get(dir) {
            return manifest_string_field(manifest, "version");
        }
        pacquet_package_manifest::safe_read_package_json_from_dir(dir)
            .ok()
            .flatten()
            .and_then(|value| value.get("version").and_then(|v| v.as_str()).map(str::to_string))
    }
}

/// Verify that linked packages are up to date: every importer
/// dependency that resolved to a workspace link must still link under
/// today's manifest spec, and every one that resolved to the registry
/// must not have become linkable. The local-file-dependency freshness
/// branch (a `file:` directory specifier) is not handled here — those
/// entries conservatively report "not up to date" so the full install
/// path re-evaluates them.
fn linked_packages_are_up_to_date(
    ctx: &LinkedPackagesContext<'_>,
    project_dir: &Path,
    manifest: &PackageManifest,
    snapshot: &ProjectSnapshot,
) -> bool {
    const GROUPS: [(DependencyGroup, &str); 3] = [
        (DependencyGroup::Optional, "optionalDependencies"),
        (DependencyGroup::Prod, "dependencies"),
        (DependencyGroup::Dev, "devDependencies"),
    ];
    for (group, manifest_field) in GROUPS {
        let Some(lockfile_deps) = snapshot.get_map_by_group(group) else {
            continue;
        };
        let Some(manifest_deps) =
            manifest.value().get(manifest_field).and_then(|value| value.as_object())
        else {
            continue;
        };
        for (dep_name, dep) in lockfile_deps {
            let dep_name = dep_name.to_string();
            let Some(current_spec) = manifest_deps.get(&dep_name).and_then(|v| v.as_str()) else {
                continue;
            };
            if ref_is_local_directory(&dep.specifier) {
                // A `file:` specifier that resolved to `link:` (e.g. an
                // injected self-reference) is a local link with no
                // `packages:` entry — up to date by construction.
                if matches!(dep.version, ImporterDepVersion::Link(_)) {
                    continue;
                }
                return false;
            }
            let link_target = dep.version.as_link_target();
            let is_linked = link_target.is_some();
            if is_linked
                && (current_spec.starts_with("link:")
                    || current_spec.starts_with("file:")
                    || current_spec.starts_with("workspace:."))
            {
                continue;
            }
            // A linked dependency whose spec is a distribution tag is
            // considered up to date to skip full resolution.
            if is_linked && spec_is_distribution_tag(current_spec) {
                continue;
            }
            let linked_dir: Option<std::borrow::Cow<'_, Path>> = match link_target {
                Some(target) => Some(std::borrow::Cow::Owned(project_dir.join(target))),
                None => dep
                    .version
                    .as_regular()
                    .map(std::string::ToString::to_string)
                    .and_then(|version| ctx.workspace_packages.get(&dep_name)?.get(&version))
                    .map(|dir| std::borrow::Cow::Borrowed(*dir)),
            };
            let Some(linked_dir) = linked_dir else {
                continue;
            };
            if !ctx.link_workspace_packages && !current_spec.starts_with("workspace:") {
                // A linkable dir exists, but nothing requests linking it.
                continue;
            }
            let available_range = version_range_of_spec(current_spec);
            let local_package_satisfies_range = matches!(available_range, "*" | "^" | "~")
                || ctx
                    .linked_version(&linked_dir)
                    .is_some_and(|version| semver_satisfies_loosely(&version, available_range));
            if is_linked != local_package_satisfies_range {
                return false;
            }
        }
    }
    true
}

/// Whether a specifier points at a local directory: a `file:`
/// specifier that is not a tarball.
fn ref_is_local_directory(specifier: &str) -> bool {
    specifier.starts_with("file:")
        && !(specifier.ends_with(".tgz")
            || specifier.ends_with(".tar.gz")
            || specifier.ends_with(".tar"))
}

/// Whether a bare specifier is an npm distribution tag (`latest`,
/// `beta`, ...): anything that doesn't parse as a semver range and
/// contains only characters a tag name may carry. Protocol-ish specs
/// (`workspace:^1.0.0`, `npm:foo@1`) contain `:`/`@`/`/` and therefore
/// never match.
fn spec_is_distribution_tag(spec: &str) -> bool {
    !spec.is_empty()
        && spec.parse::<node_semver::Range>().is_err()
        && spec.chars().all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
}

/// Strip the `workspace:` / `npm:` envelope so the remainder can be
/// compared as a semver range.
fn version_range_of_spec(spec: &str) -> &str {
    if let Some(rest) = spec.strip_prefix("workspace:") {
        return rest;
    }
    if let Some(rest) = spec.strip_prefix("npm:") {
        // `npm:<alias>@<range>` — the `@` search starts at index 1 so a
        // leading scope `@` isn't mistaken for the separator.
        return match rest.get(1..).and_then(|tail| tail.find('@')) {
            Some(at) => {
                let range = &rest[at + 2..];
                if range.is_empty() { "*" } else { range }
            }
            None => "*",
        };
    }
    spec
}

/// `semver.satisfies(version, range, { loose: true })` — a version or
/// range that doesn't parse fails the match.
fn semver_satisfies_loosely(version: &str, range: &str) -> bool {
    let Ok(version) = version.parse::<node_semver::Version>() else { return false };
    let Ok(range) = range.parse::<node_semver::Range>() else { return false };
    range.satisfies(&version)
}

/// Millisecond mtime of `path`, `None` when it can't be stat'd.
/// Converts wall-clock to ms-since-epoch the same way
/// `pacquet_workspace_state::now_millis` does on the write side, so
/// comparisons against `lastValidatedTimestamp` are apples-to-apples.
fn mtime_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).and_then(|metadata| metadata.modified()).ok()?;
    let elapsed = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

/// Whether `<workspace_root>/pnpm-lock.yaml` has an mtime newer than the
/// last validation. A lockfile-only change leaves every manifest
/// untouched but must still defeat the manifest-mtime fast path. A
/// missing lockfile reports `false` here — it is handled by the
/// existence and stand-in gates, not treated as a modification.
fn wanted_lockfile_modified(workspace_root: &Path, last_validated_timestamp: i64) -> bool {
    mtime_ms(&workspace_root.join(Lockfile::FILE_NAME))
        .is_some_and(|mtime| mtime > last_validated_timestamp)
}

fn current_lockfile_unusable_with_non_empty_wanted(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<bool, &'static str> {
    if check.is_workspace_install || !check.config.lockfile {
        return Ok(false);
    }
    if current_lockfile_file_has_content(&check.config.virtual_store_dir) {
        return Ok(false);
    }
    let Some(wanted) =
        check.lockfile.get().map_err(|_| "the wanted lockfile cannot be read or parsed")?
    else {
        return Ok(false);
    };
    Ok(!wanted.is_empty())
}

fn current_lockfile_file_has_content(virtual_store_dir: &Path) -> bool {
    fs::metadata(virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

/// Compare today's settings against what the previous install
/// recorded.
///
/// Only the fields pacquet populates via [`current_settings`]
/// participate in the comparison; the rest are listed at the end of
/// this function with the reason each is safe to skip.
///
/// pnpm iterates the full `WORKSPACE_STATE_SETTING_KEYS` list, reading a
/// key absent from the recorded state as `undefined`. So the reverse
/// scenario (pacquet wrote the state, pnpm reads it next) stays on the
/// fast path only for keys whose pnpm-resolved value is also
/// `undefined`. Every key pnpm resolves to a concrete default —
/// `excludeLinksFromLockfile` (`false`), `minimumReleaseAge` (`1440`),
/// `minimumReleaseAgeIgnoreMissingTime` (`true`) — must therefore be
/// written by [`current_settings`] and compared here, or pnpm would
/// report drift and re-run a (no-op) install on every command after a
/// pacquet install. `enableGlobalVirtualStore` is `undefined` by
/// default (concrete only under `--global`/CI), so pacquet's omit-when-
/// off encoding already matches. The `allowBuilds` coercion treats an
/// absent value as an empty map on the read side, matching pnpm's
/// tolerance of an absent `allowBuilds` key in the recorded state on
/// the write side.
fn settings_match(
    state: &WorkspaceState,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> bool {
    first_setting_drift(state, config, node_linker, included, false).is_none()
}

/// The camelCase name (pnpm's workspace-state setting key) of the first
/// recorded setting that differs from today's config, or `None` when
/// they all match. `ignore_included_groups` skips `dev` / `optional` /
/// `production`: `pnpm run` / `pnpm exec` always execute with the
/// default dependency groups, so those never match the state written by
/// a `--production` / `--no-optional` install (pnpm's
/// `ignoredWorkspaceStateSettings`).
fn first_setting_drift(
    state: &WorkspaceState,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    ignore_included_groups: bool,
) -> Option<&'static str> {
    let current = current_settings(config, node_linker, included);
    let recorded = &state.settings;
    let live = &current;
    if !allow_builds_match(recorded.allow_builds.as_ref(), live.allow_builds.as_ref()) {
        return Some("allowBuilds");
    }
    if recorded.auto_install_peers != live.auto_install_peers {
        return Some("autoInstallPeers");
    }
    if recorded.dedupe_direct_deps != live.dedupe_direct_deps {
        return Some("dedupeDirectDeps");
    }
    if recorded.dedupe_injected_deps != live.dedupe_injected_deps {
        return Some("dedupeInjectedDeps");
    }
    if recorded.dedupe_peer_dependents != live.dedupe_peer_dependents {
        return Some("dedupePeerDependents");
    }
    if recorded.dedupe_peers != live.dedupe_peers {
        return Some("dedupePeers");
    }
    if !ignore_included_groups && recorded.dev != live.dev {
        return Some("dev");
    }
    if !enable_global_virtual_store_match(
        recorded.enable_global_virtual_store,
        live.enable_global_virtual_store,
    ) {
        return Some("enableGlobalVirtualStore");
    }
    if recorded.exclude_links_from_lockfile != live.exclude_links_from_lockfile {
        return Some("excludeLinksFromLockfile");
    }
    if recorded.hoist_pattern != live.hoist_pattern {
        return Some("hoistPattern");
    }
    if recorded.hoist_workspace_packages != live.hoist_workspace_packages {
        return Some("hoistWorkspacePackages");
    }
    if recorded.ignored_optional_dependencies != live.ignored_optional_dependencies {
        return Some("ignoredOptionalDependencies");
    }
    if recorded.inject_workspace_packages != live.inject_workspace_packages {
        return Some("injectWorkspacePackages");
    }
    if recorded.link_workspace_packages != live.link_workspace_packages {
        return Some("linkWorkspacePackages");
    }
    if recorded.minimum_release_age != live.minimum_release_age {
        return Some("minimumReleaseAge");
    }
    if recorded.minimum_release_age_ignore_missing_time
        != live.minimum_release_age_ignore_missing_time
    {
        return Some("minimumReleaseAgeIgnoreMissingTime");
    }
    if recorded.node_linker != live.node_linker {
        return Some("nodeLinker");
    }
    if !ignore_included_groups && recorded.optional != live.optional {
        return Some("optional");
    }
    if recorded.overrides != live.overrides {
        return Some("overrides");
    }
    if !package_extensions_match(
        recorded.package_extensions.as_ref(),
        live.package_extensions.as_ref(),
    ) {
        return Some("packageExtensions");
    }
    if recorded.patched_dependencies != live.patched_dependencies {
        return Some("patchedDependencies");
    }
    if recorded.peers_suffix_max_length != live.peers_suffix_max_length {
        return Some("peersSuffixMaxLength");
    }
    if recorded.prefer_workspace_packages != live.prefer_workspace_packages {
        return Some("preferWorkspacePackages");
    }
    if !ignore_included_groups && recorded.production != live.production {
        return Some("production");
    }
    if recorded.public_hoist_pattern != live.public_hoist_pattern {
        return Some("publicHoistPattern");
    }
    None
    // Deliberately *not* compared in this generic settings loop:
    // `catalogs` is ignored here and checked separately in
    // `check_optimistic_repeat_install` so catalogs from either
    // `pnpm-workspace.yaml` or an `updateConfig` hook can invalidate
    // the cache.
    //
    // The remaining omitted keys are left out because pnpm leaves them
    // `undefined` by default, so omitting them here still matches pnpm's
    // all-key freshness check (`undefined == undefined`):
    //   minimumReleaseAgeStrict     (pnpm sets it only when the user
    //                                explicitly sets minimumReleaseAge)
    //   minimumReleaseAgeExclude
    //   trustPolicy*                (all `undefined` until configured)
    //   workspacePackagePatterns    (concrete for a multi-package
    //                                workspace, but lives in the
    //                                workspace manifest, not `Config`;
    //                                threading it into `current_settings`
    //                                is a separate follow-up. pacquet
    //                                detects project-set changes via
    //                                `project_structure_matches`).
}

/// `enableGlobalVirtualStore` has no `?? default` coercion on pnpm's
/// read side, but its `undefined` default and an explicit `false` both
/// mean "global virtual store off". pnpm omits the key for the former
/// and records `false` only when CI forces it; pacquet omits both.
/// Normalize the absent and `false` forms before comparing so a
/// pnpm-written file (omitted or `false`) matches a pacquet install
/// with the store off, while a real `true`/`false` flip still trips.
fn enable_global_virtual_store_match(
    state_value: Option<bool>,
    current_value: Option<bool>,
) -> bool {
    state_value.unwrap_or(false) == current_value.unwrap_or(false)
}

/// Pnpm writes `Some({})` for an empty `allowBuilds`; pacquet writes
/// `None` for the same effective value. Treat them as equivalent so
/// cross-package-manager state files don't trip the comparison.
fn allow_builds_match(
    state_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
    current_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
) -> bool {
    match (state_value, current_value) {
        (None, None) => true,
        (Some(map), None) | (None, Some(map)) => map.is_empty(),
        (Some(state_map), Some(current_map)) => state_map == current_map,
    }
}

/// `packageExtensions` are compared as opaque `serde_json::Value`
/// trees so the workspace-state file written by either implementation
/// round-trips through the other. Empty maps are equivalent to absent
/// — pacquet's [`pacquet_config::WorkspaceSettings::apply_to`] already collapses
/// `packageExtensions: {}` to `None`, but pnpm may write `Some({})`
/// directly, and the workspace-state file is shared across the two.
fn package_extensions_match(
    state_value: Option<&serde_json::Value>,
    current_value: Option<&serde_json::Value>,
) -> bool {
    fn is_empty(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.is_empty(),
            serde_json::Value::Null => true,
            _ => false,
        }
    }
    match (state_value, current_value) {
        (None, None) => true,
        (Some(value), None) | (None, Some(value)) => is_empty(value),
        (Some(state_value), Some(current_value)) => state_value == current_value,
    }
}

/// Build the [`WorkspaceStateSettings`] that today's install would
/// write. Shared with `install::build_workspace_state` so the
/// freshness check sees the same byte shape the writer produced —
/// when one side grows a field, the other automatically does too.
pub(crate) fn current_settings(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> WorkspaceStateSettings {
    let allow_builds = (!config.allow_builds.is_empty()).then(|| {
        config.allow_builds.iter().map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v))).collect()
    });
    WorkspaceStateSettings {
        allow_builds,
        auto_install_peers: Some(config.auto_install_peers),
        dedupe_direct_deps: Some(config.dedupe_direct_deps),
        dedupe_injected_deps: Some(config.dedupe_injected_deps),
        dedupe_peer_dependents: Some(config.dedupe_peer_dependents),
        dedupe_peers: Some(config.dedupe_peers),
        dev: Some(included.dev_dependencies),
        // Mirror pnpm's writer, which omits the key for its `undefined`
        // default and records a concrete value only when forced. pacquet
        // has no `--global` flow, so the only "on" value it ever writes
        // is `true`; an off store maps back to the omitted `None`.
        enable_global_virtual_store: config.enable_global_virtual_store.then_some(true),
        exclude_links_from_lockfile: Some(config.exclude_links_from_lockfile),
        hoist_pattern: config.hoist_pattern.clone(),
        hoist_workspace_packages: Some(config.hoist_workspace_packages),
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        inject_workspace_packages: Some(config.inject_workspace_packages),
        link_workspace_packages: Some(link_workspace_packages_to_json(
            config.link_workspace_packages,
        )),
        minimum_release_age: config.minimum_release_age,
        minimum_release_age_ignore_missing_time: Some(
            config.minimum_release_age_ignore_missing_time,
        ),
        node_linker: Some(map_node_linker(node_linker)),
        optional: Some(included.optional_dependencies),
        overrides: config
            .overrides
            .as_ref()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        package_extensions: config
            .package_extensions
            .as_ref()
            .and_then(|map| serde_json::to_value(map).ok()),
        patched_dependencies: config.patched_dependencies.clone(),
        peers_suffix_max_length: Some(
            u32::try_from(config.peers_suffix_max_length).unwrap_or(u32::MAX),
        ),
        prefer_workspace_packages: Some(config.prefer_workspace_packages),
        production: Some(included.dependencies),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        ..Default::default()
    }
}

pub(crate) fn current_settings_with_catalogs(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    catalogs: &Catalogs,
) -> WorkspaceStateSettings {
    let mut settings = current_settings(config, node_linker, included);
    settings.catalogs = Some(catalogs_to_json(catalogs));
    settings
}

fn catalogs_cache_matches(recorded: Option<&serde_json::Value>, current: &Catalogs) -> bool {
    let recorded = recorded.cloned().map_or_else(empty_json_object, filter_null_object_values);
    let current = filter_null_object_values(catalogs_to_json(current));
    recorded == current
}

fn catalogs_to_json(catalogs: &Catalogs) -> serde_json::Value {
    serde_json::to_value(catalogs).expect("Catalogs serialize to a JSON object")
}

fn empty_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn filter_null_object_values(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut map) = value else { return value };
    map.retain(|_, value| !value.is_null());
    serde_json::Value::Object(map)
}

fn link_workspace_packages_to_json(value: LinkWorkspacePackages) -> serde_json::Value {
    match value {
        LinkWorkspacePackages::Off => serde_json::Value::Bool(false),
        LinkWorkspacePackages::DirectOnly => serde_json::Value::Bool(true),
        LinkWorkspacePackages::Deep => serde_json::Value::String("deep".to_string()),
    }
}

fn map_node_linker(linker: NodeLinker) -> WorkspaceStateNodeLinker {
    match linker {
        NodeLinker::Isolated => WorkspaceStateNodeLinker::Isolated,
        NodeLinker::Hoisted => WorkspaceStateNodeLinker::Hoisted,
        NodeLinker::Pnp => WorkspaceStateNodeLinker::Pnp,
    }
}

/// Project count + per-project (key, name, version) match between the
/// cached state and today's walk. The key is the project's root dir;
/// `build_workspace_state` and pnpm both use it as the map key, so a
/// renamed / removed / added project trips the check immediately.
fn project_structure_matches(
    state: &WorkspaceState,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    if state.projects.len() != project_manifests.len() {
        return false;
    }
    project_manifests.iter().all(|(root_dir, manifest)| {
        let key = root_dir.to_string_lossy().into_owned();
        let Some(entry) = state.projects.get(&key) else {
            return false;
        };
        entry.name.as_deref() == manifest_string_field(manifest, "name").as_deref()
            && entry.version.as_deref().unwrap_or("0.0.0")
                == manifest_string_field(manifest, "version").as_deref().unwrap_or("0.0.0")
    })
}

fn modules_dirs_present(
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    first_project_missing_modules_dir(config, project_manifests).is_none()
}

/// The id (`name` field, falling back to the root dir) of the first
/// project that declares dependencies but has no modules directory, or
/// `None` when every project with dependencies has one.
fn first_project_missing_modules_dir(
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Option<String> {
    project_manifests.iter().find_map(|(root_dir, manifest)| {
        if !manifest_has_runtime_deps(manifest) {
            return None;
        }
        // The root importer uses `config.modules_dir`; siblings use
        // their own `<root>/node_modules`. Matches the isolated-linker
        // default — `config.modules_dir` is `<workspace_root>/node_modules`
        // unless the user overrode it explicitly.
        let modules_dir = if *root_dir == workspace_dir_of(config, root_dir) {
            config.modules_dir.clone()
        } else {
            root_dir.join("node_modules")
        };
        if modules_dir.exists() {
            return None;
        }
        Some(
            manifest_string_field(manifest, "name")
                .unwrap_or_else(|| root_dir.to_string_lossy().into_owned()),
        )
    })
}

/// Recover the workspace root from `config.modules_dir`. The root
/// importer's `root_dir` equals `config.modules_dir.parent()` because
/// `config.modules_dir` is `<workspace_root>/node_modules`. Used by
/// [`modules_dirs_present`] to tell root from sibling — a brittle
/// shape but it matches how the install path itself derives
/// `config.modules_dir`.
fn workspace_dir_of(config: &Config, fallback: &Path) -> PathBuf {
    config.modules_dir.parent().map_or_else(|| fallback.to_path_buf(), Path::to_path_buf)
}

fn manifest_has_runtime_deps(manifest: &PackageManifest) -> bool {
    let value = manifest.value();
    [value.get("dependencies"), value.get("devDependencies"), value.get("optionalDependencies")]
        .into_iter()
        .flatten()
        .any(|deps| deps.as_object().is_some_and(|map| !map.is_empty()))
}

fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest.value().get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}

/// Whether any configured patch file's mtime is newer than the last
/// validation. A patch that can't be stat'd is treated as not-modified,
/// leaving a genuinely missing patch to surface on the full install
/// path. Patch paths are resolved against `workspace_root` (the
/// `pnpm-workspace.yaml` dir, where `patchedDependencies` is declared),
/// matching how [`Config::patched_dependency_hashes`] resolves them.
fn patches_modified_since(workspace_root: &Path, config: &Config, cutoff_ms: i64) -> bool {
    let Some(patches) = config.patched_dependencies.as_ref() else {
        return false;
    };
    patches.values().any(|rel_or_abs| {
        let candidate = Path::new(rel_or_abs);
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace_root.join(candidate)
        };
        let Ok(modified) = fs::metadata(&path).and_then(|metadata| metadata.modified()) else {
            return false;
        };
        let Ok(elapsed) = modified.duration_since(SystemTime::UNIX_EPOCH) else {
            return false;
        };
        let modified_ms = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        modified_ms > cutoff_ms
    })
}

/// The pnpmfile list recorded in the workspace state and compared by
/// the freshness check: today just the workspace pnpmfile.
/// Config-dependency plugin pnpmfiles are tracked via the
/// `config_dependencies` comparison instead.
pub(crate) fn current_pnpmfiles(workspace_root: &Path) -> Vec<String> {
    pacquet_hooks::finder::find_pnpmfile(workspace_root)
        .map(|path| path.to_string_lossy().into_owned())
        .into_iter()
        .collect()
}

/// Whether the pnpmfiles changed since the last validation: the
/// recorded pnpmfile list must match the current one, every recorded
/// pnpmfile must still exist, and none may be newer than the last
/// validation.
fn pnpmfiles_modified_since(workspace_root: &Path, previous: &[String], cutoff_ms: i64) -> bool {
    pnpmfiles_drift(workspace_root, previous, cutoff_ms).is_some()
}

/// [`pnpmfiles_modified_since`] with the drift spelled out in pnpm's
/// issue wording, for the verify-deps-before-run gate's user-facing
/// messages.
fn pnpmfiles_drift(workspace_root: &Path, previous: &[String], cutoff_ms: i64) -> Option<String> {
    let current = current_pnpmfiles(workspace_root);
    if current != previous {
        return Some("The list of pnpmfiles changed.".to_string());
    }
    current.iter().find_map(|path| {
        let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
            return Some(format!(r#"pnpmfile at "{path}" was removed"#));
        };
        let Ok(elapsed) = modified.duration_since(SystemTime::UNIX_EPOCH) else {
            return Some(format!(r#"pnpmfile at "{path}" was modified"#));
        };
        let modified_ms = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        (modified_ms > cutoff_ms).then(|| format!(r#"pnpmfile at "{path}" was modified"#))
    })
}

/// Stat every project's `package.json`. `None` on any stat failure —
/// "can't prove freshness, fall through".
fn stat_manifests<'a>(
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
) -> Option<Vec<ManifestStat<'a>>> {
    project_manifests
        .iter()
        .map(|(root_dir, manifest)| {
            mtime_ms(manifest.path()).map(|mtime_ms| ManifestStat {
                root_dir: root_dir.as_path(),
                manifest,
                mtime_ms,
            })
        })
        .collect()
}

/// Outcome of [`check_deps_status_before_run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDepsStatus {
    UpToDate,
    /// `node-linker: pnp` installs cannot be inspected. The caller
    /// warns ("verify-deps-before-run does not work with
    /// node-linker=pnp") and runs the script.
    SkippedPnp,
    Outdated {
        /// pnpm's issue wording for the detected drift, shown by the
        /// `warn` and `error` actions.
        issue: String,
        /// `pnpm install` arguments reproducing the dependency groups
        /// the workspace state recorded (`--production` / `--dev` /
        /// `--no-optional`), for the `install` and `prompt` actions.
        install_args: Vec<String>,
    },
}

/// The verify-deps-before-run twin of
/// [`check_optimistic_repeat_install`]: the same freshness checks, with
/// the differences pnpm's run gate carries over its install fast path —
/// it runs regardless of `optimisticRepeatInstall`, never treats local
/// file dependencies as outdated, ignores `dev`/`optional`/`production`
/// drift (scripts always run with the default groups), compares
/// configuration dependencies, and reports drift with pnpm's
/// user-facing issue wording instead of a diagnostic-only reason.
/// `state` arrives from the caller, which already had to load it to
/// decide whether a check is possible at all (a missing state is
/// "Cannot check whether dependencies are outdated").
pub fn check_deps_status_before_run(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
) -> RunDepsStatus {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included,
        project_manifests,
        is_workspace_install,
        catalogs,
        ..
    } = check;

    let install_args = install_args_from_state(state);
    let outdated =
        |issue: String| RunDepsStatus::Outdated { issue, install_args: install_args.clone() };

    if node_linker == NodeLinker::Pnp {
        return RunDepsStatus::SkippedPnp;
    }

    if let Some(setting) = first_setting_drift(state, config, node_linker, included, true) {
        return outdated(format!("The value of the {setting} setting has changed"));
    }
    if config_dependencies_drifted(config, state) {
        return outdated("Configuration dependencies are not up to date".to_string());
    }
    if !catalogs_cache_matches(state.settings.catalogs.as_ref(), catalogs) {
        return outdated("Catalogs cache outdated".to_string());
    }
    if !project_structure_matches(state, project_manifests) {
        return outdated("The workspace structure has changed since last install".to_string());
    }
    // A filtered install legitimately leaves unselected projects
    // without a modules directory.
    if !state.filtered_install
        && let Some(id) = first_project_missing_modules_dir(config, project_manifests)
    {
        return outdated(format!(
            "Workspace package {id} has dependencies but does not have a modules directory",
        ));
    }
    if !is_workspace_install
        && !workspace_root.join(Lockfile::FILE_NAME).exists()
        && !current_lockfile_file_has_content(&config.virtual_store_dir)
    {
        return outdated(format!("Cannot find a lockfile in {}", workspace_root.display()));
    }
    if patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return outdated("Patches were modified".to_string());
    }
    if let Some(issue) =
        pnpmfiles_drift(workspace_root, &state.pnpmfiles, state.last_validated_timestamp)
    {
        return outdated(issue);
    }

    let Some(manifest_stats) = stat_manifests(project_manifests) else {
        return outdated("Cannot check whether dependencies are outdated".to_string());
    };
    let modified: Vec<&ManifestStat<'_>> = manifest_stats
        .iter()
        .filter(|stat| stat.mtime_ms > state.last_validated_timestamp)
        .collect();
    let lockfile_modified =
        wanted_lockfile_modified(workspace_root, state.last_validated_timestamp);

    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => {
            return outdated(
                "The lockfile requires dependencies but none were installed".to_string(),
            );
        }
        Ok(false) => {}
        Err(reason) => return outdated(reason.to_string()),
    }

    if modified.is_empty() && !lockfile_modified {
        return match missing_wanted_lockfile_stand_in_ok(check) {
            Ok(()) => RunDepsStatus::UpToDate,
            Err(reason) => outdated(reason),
        };
    }

    let projects_to_check: Vec<&ManifestStat<'_>> =
        if lockfile_modified { manifest_stats.iter().collect() } else { modified };
    match modified_manifests_match_lockfile(check, state, &projects_to_check) {
        Ok(_) => {
            if let Err(reason) = missing_wanted_lockfile_stand_in_ok(check) {
                return outdated(reason);
            }
            if is_workspace_install {
                let mut new_state = crate::install::build_workspace_state(
                    workspace_root,
                    config,
                    node_linker,
                    included,
                    catalogs,
                    project_manifests,
                );
                // The gate ignored `dev`/`optional`/`production` drift
                // above; writing today's (default-group) values here
                // would clobber what the last real install recorded and
                // flip its next repeat-install check into "drift".
                new_state.settings.dev = state.settings.dev;
                new_state.settings.optional = state.settings.optional;
                new_state.settings.production = state.settings.production;
                if let Err(error) = update_workspace_state(workspace_root, &new_state) {
                    tracing::warn!(
                        target: "pacquet::run",
                        ?error,
                        "Failed to refresh the workspace state after the verify-deps-before-run content check",
                    );
                }
            }
            RunDepsStatus::UpToDate
        }
        Err(reason) => outdated(reason.to_string()),
    }
}

/// Read-only twin of [`regenerate_wanted_lockfile_if_missing`] for the
/// run gate: pnpm's run-path check never writes `pnpm-lock.yaml` (only
/// the install command restores it from the current lockfile), so a
/// missing wanted lockfile passes exactly when the current lockfile can
/// stand in for it, and the check leaves the workspace untouched.
fn missing_wanted_lockfile_stand_in_ok(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<(), String> {
    if check.lockfile.is_loaded_or_on_disk() || !check.config.lockfile {
        return Ok(());
    }
    match Lockfile::load_current_from_virtual_store_dir(&check.config.virtual_store_dir) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(format!("Cannot find a lockfile in {}", check.workspace_root.display())),
        Err(_) => Err("the current lockfile cannot be loaded".to_string()),
    }
}

/// The `pnpm install` arguments reproducing the dependency groups the
/// workspace state recorded, so the `install` / `prompt` actions rerun
/// the same kind of install the project last had (pnpm's
/// `createInstallArgs`).
fn install_args_from_state(state: &WorkspaceState) -> Vec<String> {
    let settings = &state.settings;
    let mut args = Vec::new();
    let dev = settings.dev.unwrap_or(false);
    let production = settings.production.unwrap_or(false);
    if production && !dev {
        args.push("--production".to_string());
    } else if dev && !production {
        args.push("--dev".to_string());
    }
    if !settings.optional.unwrap_or(false) {
        args.push("--no-optional".to_string());
    }
    args
}

/// Whether the configuration dependencies recorded by the last install
/// differ from today's config. Both sides read an absent map as empty
/// (pnpm compares `opts.configDependencies ?? {}` against
/// `workspaceState.configDependencies ?? {}`).
fn config_dependencies_drifted(config: &Config, state: &WorkspaceState) -> bool {
    if config.config_dependencies.is_none() && state.config_dependencies.is_none() {
        return false;
    }
    let empty = std::collections::BTreeMap::new();
    config.config_dependencies.as_ref().unwrap_or(&empty)
        != state.config_dependencies.as_ref().unwrap_or(&empty)
}

#[cfg(test)]
mod tests;
