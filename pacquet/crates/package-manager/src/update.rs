use crate::{Install, InstallError, ResolvedPackages, UpdateSeedPolicy};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::{Config, matcher::create_matcher};
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::{PackageTag, PackageVersion};
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_tarball::MemCache;
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

    /// Fetching a package's `latest` tag from the registry failed while
    /// computing the new manifest range for `--latest`.
    #[display("Failed to resolve the latest version of {name}: {error}")]
    #[diagnostic(code(pacquet_package_manager::update_resolve_latest))]
    ResolveLatest {
        name: String,
        #[error(source)]
        error: pacquet_registry::RegistryError,
    },

    #[display("Failed to update the manifest: {_0}")]
    UpdateManifest(#[error(source)] PackageManifestError),

    #[display("Failed to save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),
}

/// A CLI selector split into its name pattern and optional version part.
/// Ports pnpm's
/// [`parseUpdateParam`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/commands/src/recursive.ts)
/// — `lastIndexOf('@')` with `index >= 1` so a leading scope `@` is not
/// mistaken for a version separator.
struct ParsedSelector {
    pattern: String,
    version: Option<String>,
}

fn parse_update_param(input: &str) -> ParsedSelector {
    match input.rfind('@') {
        Some(idx) if idx >= 1 => ParsedSelector {
            pattern: input[..idx].to_string(),
            version: Some(input[idx + 1..].to_string()),
        },
        _ => ParsedSelector { pattern: input.to_string(), version: None },
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
            include_direct,
            depth,
            supported_architectures,
            lockfile_only,
        } = self;

        let selectors: Vec<ParsedSelector> =
            packages.iter().map(|p| parse_update_param(p)).collect();

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
            if latest {
                for (name, group, _) in &direct {
                    let version = fetch_latest(name, http_client, config).await?;
                    rewrites.push((name.clone(), *group, version.serialize(save_exact)));
                }
            }
            for (name, _, _) in &direct {
                drop_names.insert(name.clone());
            }
            // `pnpm update` (no selectors, no group narrowing) re-resolves
            // the whole graph to highest-in-range. Once the groups are
            // narrowed (`--prod` / `--dev` / `--no-optional`) only the
            // included direct deps (and their same-named transitive
            // occurrences) re-resolve.
            if updates_all_groups {
                UpdateSeedPolicy::DropAll
            } else {
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
            for (name, group, _) in &direct {
                if !matcher.matches(name) {
                    continue;
                }
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
        };

        // Apply the manifest rewrites in memory before resolving so the
        // install picks the new ranges. The save happens after the
        // install succeeds, matching pnpm's manifest-write ordering.
        let manifest_changed = !rewrites.is_empty();
        for (name, group, spec) in &rewrites {
            manifest.add_dependency(name, spec, *group).map_err(UpdateError::UpdateManifest)?;
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
        }
        .run::<Reporter>()
        .await
        .map_err(UpdateError::Install)?;

        if manifest_changed {
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
