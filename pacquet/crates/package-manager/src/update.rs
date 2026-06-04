use crate::{
    CatalogDecision, CatalogModeDep, CatalogVersionMismatchError, Install, InstallError,
    ResolvedPackages, UpdateSeedPolicy, decide_catalog,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::{CatalogMode, Config, matcher::create_matcher};
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::{PackageTag, PackageVersion};
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_tarball::MemCache;
use pacquet_workspace_manifest_writer::{UpdateWorkspaceManifestError, update_workspace_manifest};
use std::{collections::HashSet, sync::Arc};

/// The three dependency groups `pacquet update` considers as "direct"
/// targets, in the order pnpm's `updateProjectManifest` walks them.
const DIRECT_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];

/// Everything `pacquet update` (alias `up` / `upgrade`) does.
///
/// Ports pnpm's
/// [`update` command](https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/update/index.ts)
/// onto pacquet's always-fresh-resolve install path. The two halves of
/// pnpm's behavior map as follows:
///
/// * **Compatible bump** (no `--latest`): the matched names have their
///   lockfile pins withheld from the preferred-versions seed
///   ([`UpdateSeedPolicy`]) so the resolver re-picks the highest version
///   satisfying the manifest range. `package.json` is left untouched —
///   pnpm only rewrites the manifest for deps marked `updateSpec`, which
///   compatible updates are not.
/// * **`--latest`**: each matched *direct* dependency's `latest` tag is
///   fetched and written into `package.json` (`^<version>`, or the exact
///   version under `--save-exact`), exactly as `pacquet add` records a
///   freshly-added range. The follow-up install then resolves the new
///   range. Mirrors pnpm's `updateToLatest` + `updateSpec` path.
///
/// Selector handling mirrors pnpm's
/// [`update`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/update/index.ts#L282-L328):
/// bare-name selectors (`foo`, `@scope/bar-*`) with `depth > 0` and no
/// `--latest` match every package of that name **at any depth** (the
/// match is applied against the lockfile's package names, like pnpm's
/// `updateMatching(infoFromLockfile.name, ...)`); selectors carrying a
/// version (`foo@2`) or any selector under `--latest` match only direct
/// dependencies, and the version (or fetched latest) is written into the
/// manifest before resolving.
#[must_use]
pub struct Update<'a> {
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub http_client_arc: Arc<ThrottledClient>,
    pub config: &'static Config,
    pub manifest: &'a mut PackageManifest,
    pub lockfile: Option<&'a Lockfile>,
    pub lockfile_path: Option<&'a std::path::Path>,
    /// Package selectors from the CLI (`foo`, `@scope/bar-*`, `foo@2`).
    /// Empty means "update every direct dependency in the included
    /// groups", matching `pnpm update` with no arguments.
    pub packages: &'a [String],
    /// `--latest` / `-L`: ignore the manifest range and bump matched
    /// direct dependencies to their `latest` dist-tag, rewriting
    /// `package.json`.
    pub latest: bool,
    /// `--save-exact` / `-E`: write the resolved version without a range
    /// operator when rewriting the manifest under `--latest`.
    pub save_exact: bool,
    /// `--save` (default) / `--no-save`. When `false`, the manifest is
    /// not persisted: the `--latest` / versioned-selector range rewrites
    /// still drive resolution (so `pnpm-lock.yaml` updates) but
    /// `package.json` on disk is left untouched. Mirrors pnpm's
    /// `updatePackageManifest: opts.save !== false`.
    pub save: bool,
    /// Dependency groups the update considers when choosing which direct
    /// dependencies to match. Mirrors pnpm's `includeDirect` derived from
    /// `--prod` / `--dev` / `--no-optional`. Note: the *materialized*
    /// dependency set is always all three groups (pnpm's `include` is
    /// all-true for updates so the `node_modules` layout is unchanged);
    /// this only narrows the update scope.
    pub include_direct: Vec<DependencyGroup>,
    /// `--depth`. Only its `> 0` predicate is consulted (matching pnpm's
    /// `depth > 0` gate on the name matcher); `usize::MAX` stands in for
    /// pnpm's `Infinity` default.
    pub depth: usize,
    /// CLI-merged `supportedArchitectures`, forwarded to the install.
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    /// `--lockfile-only`: re-resolve and rewrite `pnpm-lock.yaml` without
    /// materializing `node_modules`. Forwarded to the install.
    pub lockfile_only: bool,
}

/// Error type of [`Update`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum UpdateError {
    /// `--latest` was combined with a versioned selector (`foo@2`).
    /// Mirrors pnpm's `ERR_PNPM_LATEST_WITH_SPEC`.
    #[display("Specs are not allowed to be used with --latest ({_0})")]
    #[diagnostic(code(ERR_PNPM_LATEST_WITH_SPEC))]
    LatestWithSpec(#[error(not(source))] String),

    /// Package selectors were given (with `--depth 0` and without
    /// `--latest`) but none matched a direct dependency. Mirrors pnpm's
    /// `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES`.
    #[display("None of the specified packages were found in the dependencies.")]
    #[diagnostic(code(ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES))]
    NoPackageInDependencies,

    /// Fetching a package's `latest` tag from the registry failed while
    /// computing the new manifest range for `--latest`.
    #[display("Failed to resolve the latest version of {name}: {error}")]
    #[diagnostic(code(pacquet_package_manager::update_resolve_latest))]
    ResolveLatest {
        name: String,
        #[error(source)]
        error: pacquet_registry::RegistryError,
    },

    /// Locating the workspace root (to read `pnpm-workspace.yaml`'s
    /// catalogs) failed while applying `catalogMode`.
    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pacquet_workspace::FindWorkspaceDirError),

    /// Reading `pnpm-workspace.yaml` failed while applying `catalogMode`.
    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] pacquet_workspace::ReadWorkspaceManifestError),

    /// `pnpm-workspace.yaml`'s catalog sections are misconfigured.
    #[diagnostic(transparent)]
    InvalidCatalogsConfiguration(#[error(source)] InvalidCatalogsConfigurationError),

    /// `catalogMode: strict` and an updated version disagreed with the
    /// catalog entry for that package.
    #[diagnostic(transparent)]
    CatalogVersionMismatch(#[error(source)] CatalogVersionMismatchError),

    /// Writing the auto-cataloged entries back to `pnpm-workspace.yaml`
    /// failed.
    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] UpdateWorkspaceManifestError),

    #[display("Failed to update the manifest: {_0}")]
    UpdateManifest(#[error(source)] PackageManifestError),

    #[display("Failed to save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),
}

/// A CLI selector split into its name pattern and optional version part.
/// Ports pnpm's
/// [`parseUpdateParam`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/recursive.ts#L582-L595):
/// the version separator is the **first** `@` at or after index `1`
/// (`2` for a `!`-negated pattern), so neither a leading scope `@` nor
/// the `!@scope/...` negation form is mistaken for a version.
struct ParsedSelector {
    pattern: String,
    version: Option<String>,
}

fn parse_update_param(input: &str) -> ParsedSelector {
    let search_start = if input.starts_with('!') { 2 } else { 1 };
    let at_index = input
        .get(search_start..)
        .and_then(|rest| rest.find('@'))
        .map(|offset| offset + search_start);
    match at_index {
        Some(idx) => ParsedSelector {
            pattern: input[..idx].to_string(),
            version: Some(input[idx + 1..].to_string()),
        },
        None => ParsedSelector { pattern: input.to_string(), version: None },
    }
}

impl Update<'_> {
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), UpdateError> {
        let Update {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            packages,
            latest,
            save_exact,
            save,
            include_direct,
            depth,
            supported_architectures,
            lockfile_only,
        } = self;

        let selectors: Vec<ParsedSelector> =
            packages.iter().map(|input| parse_update_param(input)).collect();

        // `--latest` forbids versioned selectors, matching pnpm's
        // `LATEST_WITH_SPEC` guard.
        if latest {
            let with_spec: Vec<&str> = packages
                .iter()
                .zip(&selectors)
                .filter(|(_, sel)| sel.version.is_some())
                .map(|(raw, _)| raw.as_str())
                .collect();
            if !with_spec.is_empty() {
                return Err(UpdateError::LatestWithSpec(with_spec.join(", ")));
            }
        }

        // Snapshot the direct dependencies in the included groups before
        // any manifest mutation, so the matcher and the `--latest`
        // rewrite both see the pre-update shape.
        let direct: Vec<(String, DependencyGroup, String)> = include_direct
            .iter()
            .flat_map(|&group| {
                manifest
                    .dependencies([group])
                    .map(move |(name, spec)| (name.to_string(), group, spec.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect();

        let updates_all_groups = DIRECT_GROUPS.iter().all(|group| include_direct.contains(group));

        // Names whose lockfile pins to withhold so they re-resolve, and
        // the per-direct-dep manifest rewrites (`--latest` / versioned
        // selector only).
        let mut drop_names: HashSet<String> = HashSet::new();
        let mut rewrites: Vec<(String, DependencyGroup, String)> = Vec::new();

        // Mirror pnpm's gate for the name matcher: bare-name selectors,
        // `depth > 0`, and no `--latest` use `updateMatching`, applied to
        // every package name at any depth.
        let use_name_matcher = !selectors.is_empty()
            && selectors.iter().all(|sel| sel.version.is_none())
            && depth > 0
            && !latest;

        let seed_policy = if selectors.is_empty() {
            // `updateConfig.ignoreDependencies` applies only when the user
            // gave no selectors: the listed name globs are excluded from
            // the update so they keep their lockfile pins. The filter runs
            // against the *included* direct deps, so group narrowing
            // (`--prod` / `--dev` / `--no-optional`) still scopes the
            // update. Mirrors pnpm's `makeIgnorePatterns` feeding
            // `matchDependencies(..., includeDirect)`.
            let ignore_patterns =
                config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
            // Only compile a matcher when there's something to ignore, so
            // the common (no ignore list) path skips it entirely.
            let ignore_matcher =
                (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
            let is_ignored =
                |name: &str| ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name));

            for (name, group, _) in &direct {
                if is_ignored(name) {
                    continue;
                }
                if latest {
                    let version = fetch_latest(name, http_client, config).await?;
                    rewrites.push((name.clone(), *group, version.serialize(save_exact)));
                }
                drop_names.insert(name.clone());
            }

            if updates_all_groups && ignore_patterns.is_empty() {
                // `pnpm update` (no selectors, no narrowing, no ignore
                // list) re-resolves the whole graph to highest-in-range.
                UpdateSeedPolicy::DropAll
            } else if updates_all_groups {
                // Whole-graph update minus the ignored names: drop every
                // locked name that isn't ignored so the ignored ones keep
                // their pins.
                //
                // Skip the expansion when `--latest` selected no direct
                // dependency (every included direct dep was ignored):
                // pnpm returns early there (`if (opts.latest) return`), a
                // true no-op. A non-`--latest` update with no direct match
                // still re-resolves the non-ignored *indirect* deps,
                // matching pnpm's "updating indirect dependencies only"
                // branch, so the expansion must run in that case.
                // <https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/installDeps.ts#L355-L364>
                if !(latest && drop_names.is_empty())
                    && let Some(snapshots) = lockfile.and_then(|lf| lf.snapshots.as_ref())
                {
                    for key in snapshots.keys() {
                        let name = key.name.to_string();
                        if !is_ignored(&name) {
                            drop_names.insert(name);
                        }
                    }
                }
                UpdateSeedPolicy::DropOnly(drop_names)
            } else {
                // Group-narrowed: only the included direct deps (minus
                // ignored) and their same-named transitive occurrences
                // re-resolve.
                UpdateSeedPolicy::DropOnly(drop_names)
            }
        } else if use_name_matcher {
            let patterns: Vec<String> = selectors.iter().map(|sel| sel.pattern.clone()).collect();
            let matcher = create_matcher(&patterns);
            for (name, _, _) in &direct {
                if matcher.matches(name) {
                    drop_names.insert(name.clone());
                }
            }
            // Match against every locked package name too, so a selector
            // that names a transitive-only dependency still bumps it —
            // pnpm applies `updateMatching` to `infoFromLockfile.name`.
            if let Some(snapshots) = lockfile.and_then(|lf| lf.snapshots.as_ref()) {
                for key in snapshots.keys() {
                    let name = key.name.to_string();
                    if matcher.matches(&name) {
                        drop_names.insert(name);
                    }
                }
            }
            UpdateSeedPolicy::DropOnly(drop_names)
        } else {
            // Versioned selectors and/or `--latest`: match direct
            // dependencies only and write the new range into the
            // manifest, mirroring pnpm's `matchDependencies` + `updateSpec`.
            let patterns: Vec<String> = selectors.iter().map(|sel| sel.pattern.clone()).collect();
            let matcher = create_matcher(&patterns);
            let matched_direct: Vec<(String, DependencyGroup)> = direct
                .iter()
                .filter(|(name, _, _)| matcher.matches(name))
                .map(|(name, group, _)| (name.clone(), *group))
                .collect();

            if matched_direct.is_empty() {
                // No direct dependency matched the selectors. Mirrors
                // pnpm's `matchDependencies` returning empty:
                // <https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/installDeps.ts#L353-L366>.
                if latest {
                    // `--latest` with an unmatched selector is a no-op.
                    return Ok(());
                }
                if depth == 0 {
                    return Err(UpdateError::NoPackageInDependencies);
                }
                // `depth > 0`: update only the matching *indirect*
                // dependencies (no manifest rewrite). Reached here only
                // for versioned selectors — bare-name selectors with
                // `depth > 0` take the name-matcher branch above.
                if let Some(snapshots) = lockfile.and_then(|lf| lf.snapshots.as_ref()) {
                    for key in snapshots.keys() {
                        let name = key.name.to_string();
                        if matcher.matches(&name) {
                            drop_names.insert(name);
                        }
                    }
                }
                UpdateSeedPolicy::DropOnly(drop_names)
            } else {
                for (name, group) in &matched_direct {
                    drop_names.insert(name.clone());
                    if latest {
                        let version = fetch_latest(name, http_client, config).await?;
                        rewrites.push((name.clone(), *group, version.serialize(save_exact)));
                    } else if let Some(spec) = selectors
                        .iter()
                        .find(|sel| matcher_one(&sel.pattern).matches(name))
                        .and_then(|sel| sel.version.clone())
                    {
                        rewrites.push((name.clone(), *group, spec));
                    }
                }
                UpdateSeedPolicy::DropOnly(drop_names)
            }
        };

        // Reconcile the about-to-be-written versions against the workspace
        // catalogs under `catalogMode`, mirroring pnpm's gate in
        // `installSome` plus the auto-cataloging that follows it: a matching
        // (or not-yet-cataloged) version is rewritten to `catalog:` and
        // recorded for write-back to `pnpm-workspace.yaml`. Only the
        // rewritten deps carry a user-chosen version, so a bare `update`
        // (compatible bump) produces no rewrites and nothing to reconcile.
        let mut updated_catalogs: Catalogs = Catalogs::new();
        let mut workspace_dir_for_catalogs = None;
        if config.catalog_mode != CatalogMode::Manual && !rewrites.is_empty() {
            let manifest_dir =
                manifest.path().parent().expect("manifest path always has a parent dir");
            let workspace_dir_opt = pacquet_workspace::find_workspace_dir(manifest_dir)
                .map_err(UpdateError::FindWorkspaceDir)?;
            let workspace_manifest = match workspace_dir_opt.as_deref() {
                Some(dir) => pacquet_workspace::read_workspace_manifest(dir)
                    .map_err(UpdateError::ReadWorkspaceManifest)?,
                None => None,
            };
            let catalogs = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
                .map_err(UpdateError::InvalidCatalogsConfiguration)?;
            let prefix =
                workspace_dir_opt.as_deref().unwrap_or(manifest_dir).to_string_lossy().into_owned();

            let mut reconciled: Vec<(String, DependencyGroup, String)> =
                Vec::with_capacity(rewrites.len());
            for (name, group, spec) in rewrites {
                let prev = direct
                    .iter()
                    .find(|(prev_name, prev_group, _)| *prev_name == name && *prev_group == group)
                    .map(|(_, _, prev_spec)| prev_spec.as_str());

                // `--latest` on a dependency already pinned to a catalog
                // keeps the manifest's `catalog:` reference and bumps the
                // catalog entry itself, matching pnpm's update of a
                // `catalog:` dep (the manifest bareSpecifier stays
                // `catalog:<name>`; the resolved version flows to the
                // catalog).
                if latest && let Some(catalog_name) = prev.and_then(parse_catalog_protocol) {
                    updated_catalogs
                        .entry(catalog_name.to_string())
                        .or_default()
                        .insert(name.clone(), spec);
                    continue;
                }

                let dep = CatalogModeDep {
                    alias: name.as_str(),
                    bare_specifier: spec.as_str(),
                    prev_specifier: prev,
                };
                match decide_catalog::<Reporter>(
                    config.catalog_mode,
                    None,
                    &catalogs,
                    &dep,
                    &prefix,
                )
                .map_err(UpdateError::CatalogVersionMismatch)?
                {
                    CatalogDecision::KeepDirect => reconciled.push((name, group, spec)),
                    CatalogDecision::Catalog { manifest_specifier, updated_entry } => {
                        if let Some(entry) = updated_entry {
                            updated_catalogs
                                .entry(entry.catalog_name)
                                .or_default()
                                .insert(name.clone(), entry.specifier);
                        }
                        reconciled.push((name, group, manifest_specifier));
                    }
                }
            }
            rewrites = reconciled;
            workspace_dir_for_catalogs =
                workspace_dir_opt.or_else(|| Some(manifest_dir.to_path_buf()));
        }

        // Apply the manifest rewrites in memory before resolving so the
        // install picks the new ranges. Under `--no-save` the in-memory
        // mutation still drives resolution (so `pnpm-lock.yaml` updates)
        // but the manifest is not persisted below — matching pnpm's
        // `updatePackageManifest: opts.save !== false`.
        let persist_manifest = save && !rewrites.is_empty();
        for (name, group, spec) in &rewrites {
            manifest.add_dependency(name, spec, *group).map_err(UpdateError::UpdateManifest)?;
        }

        // Write the new catalog entries to `pnpm-workspace.yaml` before the
        // install so the resolver reads them back and the lockfile's
        // `catalogs:` snapshot reflects the resolved versions.
        if !updated_catalogs.is_empty()
            && let Some(workspace_dir) = workspace_dir_for_catalogs
        {
            update_workspace_manifest(&workspace_dir, &updated_catalogs)
                .map_err(UpdateError::WriteWorkspaceManifest)?;
        }

        Install {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            // `include` is always all-true for updates: the materialized
            // `node_modules` layout must not change just because the
            // update scope was narrowed. Mirrors pnpm's update `include`.
            dependency_groups: DIRECT_GROUPS,
            frozen_lockfile: false,
            // `update` always re-resolves against the registry, so the
            // auto-frozen / repeat-install fast paths must not fire.
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            // A targeted `pacquet update <pkg>` is a partial install
            // (pnpm's `installSome`); a bare `pacquet update` is a full
            // install that runs the project's own lifecycle scripts.
            is_full_install: packages.is_empty(),
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
            update_seed_policy: seed_policy,
            auth_override: None,
        }
        .run::<Reporter>()
        .await
        .map_err(UpdateError::Install)?;

        if persist_manifest {
            manifest.save().map_err(UpdateError::SaveManifest)?;

            let prefix = manifest
                .path()
                .parent()
                .unwrap_or_else(|| manifest.path())
                .to_string_lossy()
                .into_owned();
            Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
                level: LogLevel::Debug,
                message: PackageManifestMessage::Updated {
                    prefix,
                    updated: manifest.value().clone(),
                },
            }));
        }

        Ok(())
    }
}

/// Compile a single pattern into a matcher. Used to map a matched direct
/// dependency back to the selector that claimed it (so a versioned
/// selector's version is applied to the right dep).
fn matcher_one(pattern: &str) -> pacquet_config::matcher::Matcher {
    create_matcher(std::slice::from_ref(&pattern.to_string()))
}

/// Fetch a package's `latest` dist-tag from the registry. Shares the
/// shape `pacquet add` uses for a freshly-added dependency.
async fn fetch_latest(
    name: &str,
    http_client: &ThrottledClient,
    config: &Config,
) -> Result<PackageVersion, UpdateError> {
    PackageVersion::fetch_from_registry(
        name,
        PackageTag::Latest,
        http_client,
        &config.registry,
        &config.auth_headers,
    )
    .await
    .map_err(|error| UpdateError::ResolveLatest { name: name.to_string(), error })
}

#[cfg(test)]
mod tests;
