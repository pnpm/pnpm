pub(super) use candidates::current_versions_from_importer;

use super::{
    Arc, CatalogAnchor, CatalogResolutionResult, CatalogWantedDependency, Catalogs, Config, Cow,
    DefaultResolver, DependencyGroup, HashMap, LatestQuery, Lockfile, Matcher, PackageManifest,
    PickPolicy, ResolveOptions, ResolverWantedDependency, ThrottledClient, Version,
    VersionsOverrider, configured_catalogs, create_configured_registry_resolver, create_matcher,
    github_actions, parse_catalog_protocol as dereference_catalog_protocol, parse_overrides_iter,
    project_dir, resolve_from_catalog,
};
use crate::cli_args::{
    ecosystem_deps::{
        EcosystemFilterKind, collect_ecosystem_outdated, ecosystem_project_name,
        patterns_for_ecosystem,
    },
    engine_deps::{DeclaredEngineDep, declared_engine_deps, min_version_of_spec},
};
use candidates::{collect_importer_candidates, outdated_dependency};

mod candidates;

/// State shared by every importer inspected in one `outdated` (or
/// `update --interactive`) run: one resolver and metadata cache, so a
/// dependency several workspace projects share is fetched once, plus the
/// catalogs their `catalog:` specifiers dereference against.
pub(crate) struct OutdatedRun {
    pub(crate) config: Config,
    pub(crate) http_client: Arc<ThrottledClient>,
    pub(crate) raw_packages: Vec<String>,
    pub(super) resolver: DefaultResolver,
    pub(super) resolve_options: ResolveOptions,
    pub(super) catalogs: Catalogs,
}

impl OutdatedRun {
    pub(crate) fn new(
        config: &Config,
        http_client: Arc<ThrottledClient>,
        query: &OutdatedQuery<'_>,
    ) -> miette::Result<Self> {
        let mut policy = PickPolicy::from_config(config).map_err(miette::Report::new)?;
        if query.full_metadata {
            policy.force_unfiltered_full_metadata();
        }
        let resolver =
            create_configured_registry_resolver(config, Arc::clone(&http_client), &policy)
                .map_err(miette::Report::new)?;
        Ok(Self {
            config: config.clone(),
            http_client,
            raw_packages: Vec::new(),
            resolver,
            resolve_options: ResolveOptions {
                version: pnpm_resolving_resolver_base::VersionSelectionOptions {
                    default_tag: Some("latest".to_string()),
                    ..Default::default()
                },
                policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
                    published_by: policy.published_by,
                    published_by_exclude: policy.published_by_exclude,
                    ..Default::default()
                },
                ..ResolveOptions::default()
            },
            catalogs: configured_catalogs(config)?,
        })
    }

    #[must_use]
    pub(crate) fn with_raw_packages(mut self, raw_packages: Vec<String>) -> Self {
        self.raw_packages = raw_packages;
        self
    }

    /// `manifest` with the overrides `lockfile` records applied.
    fn overridden_manifest<'a>(
        &self,
        manifest: &'a PackageManifest,
        lockfile: Option<&Lockfile>,
    ) -> miette::Result<Cow<'a, PackageManifest>> {
        let Some(overrides) = lockfile.and_then(|lockfile| lockfile.overrides.as_ref()) else {
            return Ok(Cow::Borrowed(manifest));
        };
        let overrides =
            parse_overrides_iter(overrides, &self.catalogs).map_err(miette::Report::new)?;
        // Only registry specifiers are compared, so the base a `link:`
        // override resolves against does not matter here.
        let project_dir = project_dir(manifest);
        let mut overridden = manifest.clone();
        VersionsOverrider::new(&overrides, project_dir).apply(&mut overridden, Some(project_dir));
        Ok(Cow::Owned(overridden))
    }
}

/// Which registry version a dependency is compared against to decide
/// whether it is outdated.
#[derive(Debug, Clone, Copy)]
pub enum TargetVersion {
    /// The `latest` dist-tag — the absolute newest published version.
    Latest,
    /// The highest version satisfying the manifest range. pnpm's
    /// `outdated --compatible`, and the version an in-range `update`
    /// would move to.
    WithinRange,
}

/// A direct dependency with a newer (or deprecated) registry version.
///
/// `current` is the lockfile-pinned version; `target` is the resolved
/// [`TargetVersion`]. Both are always present — dependencies without a
/// lockfile pin, without a registry target, or whose specifier is not a
/// supported registry specifier are dropped during collection because they cannot
/// be diffed.
pub struct OutdatedPackage {
    /// The `package.json` key (and `node_modules` directory name). Equals
    /// `package_name` except for aliased registry entries (`"foo": "npm:bar@^1"`).
    pub alias: String,
    /// The registry package name actually queried.
    pub package_name: String,
    pub belongs_to: DependencyGroup,
    pub current: Version,
    pub target: Version,
    pub wanted: Version,
    pub github_action: bool,
    pub metadata: OutdatedMetadata,
}

impl OutdatedPackage {
    #[must_use]
    pub fn current_str(&self) -> Cow<'_, str> {
        self.metadata.display_versions
            .as_ref()
            .map_or_else(
                || Cow::Owned(self.current.to_string()),
                |versions| Cow::Borrowed(versions.current.as_str()),
            )
    }

    #[must_use]
    pub fn target_str(&self) -> Cow<'_, str> {
        self.metadata.display_versions
            .as_ref()
            .map_or_else(
                || Cow::Owned(self.target.to_string()),
                |versions| Cow::Borrowed(versions.target.as_str()),
            )
    }

    #[must_use]
    pub fn wanted_str(&self) -> Cow<'_, str> {
        self.metadata.display_versions
            .as_ref()
            .map_or_else(
                || Cow::Owned(self.wanted.to_string()),
                |versions| Cow::Borrowed(versions.wanted.as_str()),
            )
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct OutdatedDisplayVersions {
    pub current: String,
    pub target: String,
    pub wanted: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct OutdatedMetadata {
    /// Deprecation reason of the `target` version, when the registry
    /// marked it deprecated.
    pub deprecated: Option<String>,
    /// `homepage` of the package, shown in the `--long` details column
    /// when the registry serves it.
    pub homepage: Option<String>,
    /// Name of the workspace project this dependency was found in, shown
    /// in the interactive update list's `Workspace` column. `None` for a
    /// project without a `name`, and for entries that belong to no
    /// project (GitHub Actions).
    pub workspace: Option<String>,
    /// Exact version strings to display when the ecosystem's version format
    /// differs from 3-component semver (e.g. PEP 440 `"1.0"`).
    pub display_versions: Option<Box<OutdatedDisplayVersions>>,
}

impl From<github_actions::OutdatedGitHubAction> for OutdatedPackage {
    fn from(action: github_actions::OutdatedGitHubAction) -> Self {
        Self {
            alias: action.name.clone(),
            package_name: action.name,
            belongs_to: DependencyGroup::Dev,
            current: action.current,
            target: action.latest,
            wanted: action.wanted,
            github_action: true,
            metadata: OutdatedMetadata {
                deprecated: None,
                homepage: Some(action.homepage),
                workspace: None,
                display_versions: None,
            },
        }
    }
}

/// What counts as outdated for a [`collect_outdated`] run.
pub struct OutdatedQuery<'a> {
    /// The registry version each dependency is compared against.
    pub target_version: TargetVersion,
    /// Dependency groups to inspect.
    pub include_direct: &'a [DependencyGroup],
    /// When present, restricts the walk to dependency keys the matcher
    /// accepts (pnpm's `outdated <pattern>` arguments).
    pub match_names: Option<&'a Matcher>,
    /// When present, drops the dependency keys the matcher accepts —
    /// `updateConfig.ignoreDependencies`, which takes a dependency out of
    /// both the report and the interactive update list.
    pub ignore_names: Option<&'a Matcher>,
    /// Also report a dependency whose `target` is deprecated even when it
    /// is not strictly newer than `current`. `outdated` sets this;
    /// `update` does not (a deprecated-but-current dependency has no
    /// newer version to move to).
    pub include_deprecated: bool,
    /// Fetch the full packument instead of the abbreviated install
    /// metadata. The full document is the only source of `homepage`,
    /// which the `--long` details column renders.
    pub full_metadata: bool,
}

impl OutdatedQuery<'_> {
    pub(crate) fn matches_name(&self, name: &str) -> bool {
        self.match_names.is_none_or(|matcher| matcher.matches(name))
            && !self.ignore_names.is_some_and(|matcher| matcher.matches(name))
    }
}

/// The matcher for `updateConfig.ignoreDependencies`, or [`None`] when
/// nothing is ignored.
pub(crate) fn ignored_dependencies_matcher(config: &Config) -> Option<Matcher> {
    config.update_config.ignore_dependencies
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(create_matcher)
}

/// Gather the direct dependencies whose `target` version is newer than
/// the lockfile-pinned `current` version (or, per
/// [`OutdatedQuery::include_deprecated`], whose `target` is deprecated).
///
/// Registry failures abort the query with package context; dependencies whose
/// metadata has no compatible target are omitted from the result.
pub async fn collect_outdated(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    query: &OutdatedQuery<'_>,
) -> miette::Result<Vec<OutdatedPackage>> {
    collect_outdated_for_importer(
        manifest,
        lockfile,
        Lockfile::ROOT_IMPORTER_KEY,
        config,
        http_client,
        query,
    )
    .await
}

pub(crate) async fn collect_outdated_for_importer(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    query: &OutdatedQuery<'_>,
) -> miette::Result<Vec<OutdatedPackage>> {
    collect_outdated_for_importer_in_run(
        manifest,
        lockfile,
        importer_id,
        query,
        &OutdatedRun::new(config, Arc::clone(http_client), query)?,
    )
    .await
}

pub(crate) async fn collect_outdated_for_importer_in_run(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    query: &OutdatedQuery<'_>,
    run: &OutdatedRun,
) -> miette::Result<Vec<OutdatedPackage>> {
    let manifest = &run.overridden_manifest(manifest, lockfile)?;
    let workspace = importer_workspace_label(manifest, importer_id, &run.config);
    let mut results =
        collect_node_outdated_for_importer(manifest, lockfile, importer_id, &workspace, query, run)
            .await?;
    if run.config.cargo.enabled || run.config.python.enabled {
        let ecosystem_outdated = Box::pin(collect_ecosystem_outdated(
            project_dir(manifest),
            Some(workspace),
            query,
            &run.raw_packages,
            run,
        ))
        .await?;
        results.extend(ecosystem_outdated);
    }
    Ok(results)
}

async fn collect_node_outdated_for_importer(
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    workspace: &str,
    query: &OutdatedQuery<'_>,
    run: &OutdatedRun,
) -> miette::Result<Vec<OutdatedPackage>> {
    let node_patterns = patterns_for_ecosystem(&run.raw_packages, EcosystemFilterKind::Node)?;
    if !run.raw_packages.is_empty() && node_patterns.is_empty() {
        return Ok(Vec::new());
    }
    let node_matcher = (!node_patterns.is_empty()).then(|| create_matcher(&node_patterns));
    let node_query =
        OutdatedQuery { match_names: node_matcher.as_ref().or(query.match_names), ..*query };
    let current_versions =
        current_versions_from_importer(lockfile, importer_id, node_query.include_direct);
    let engine_deps = declared_engine_deps(manifest, &run.config, node_query.include_direct);
    let candidates =
        collect_importer_candidates(manifest, &current_versions, &engine_deps, &node_query);
    let fetches = candidates
        .into_iter()
        .map(|candidate| outdated_dependency(run, &node_query, workspace, candidate));
    let fetched = futures_util::future::join_all(fetches).await
        .into_iter()
        .collect::<miette::Result<Vec<_>>>()?;
    Ok(fetched.into_iter().flatten().collect())
}

fn importer_workspace_label(
    manifest: &PackageManifest,
    importer_id: &str,
    config: &Config,
) -> String {
    manifest
        .value()
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| ecosystem_project_name(project_dir(manifest), config))
        .unwrap_or_else(|| importer_id.to_string())
}
