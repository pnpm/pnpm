use super::{
    Arc, CatalogResolutionResult, CatalogWantedDependency, Catalogs, Config, Cow, DependencyGroup,
    HashMap, InMemoryPackageMetaCache, LatestQuery, Lockfile, Matcher, NpmResolver,
    PackageManifest, PickPolicy, ResolveOptions, ResolverWantedDependency, ThrottledClient,
    Version, configured_catalogs, create_configured_npm_resolver, create_matcher, github_actions,
    parse_catalog_protocol, resolve_from_catalog,
};
use pnpm_resolving_resolver_base::Resolver;

/// State shared by every importer inspected in one `outdated` (or
/// `update --interactive`) run: one resolver and metadata cache, so a
/// dependency several workspace projects share is fetched once, plus the
/// catalogs their `catalog:` specifiers dereference against.
pub(crate) struct OutdatedRun {
    pub(super) resolver: NpmResolver<InMemoryPackageMetaCache>,
    pub(super) resolve_options: ResolveOptions,
    pub(super) catalogs: Catalogs,
}

impl OutdatedRun {
    pub(crate) fn new(config: &Config, http_client: Arc<ThrottledClient>) -> miette::Result<Self> {
        let policy = PickPolicy::from_config(config).map_err(miette::Report::new)?;
        let resolver = create_configured_npm_resolver(config, http_client, &policy)
            .map_err(miette::Report::new)?;
        Ok(Self {
            resolver,
            resolve_options: ResolveOptions {
                default_tag: Some("latest".to_string()),
                published_by: policy.published_by,
                published_by_exclude: policy.published_by_exclude,
                ..ResolveOptions::default()
            },
            catalogs: configured_catalogs(config)?,
        })
    }
}

/// Which registry version a dependency is compared against to decide
/// whether it is outdated.
#[derive(Debug, Clone, Copy)]
pub enum TargetVersion {
    /// The `latest` dist-tag — the absolute newest published version.
    /// pnpm's default for `outdated`.
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
/// plain semver range are dropped during collection because they cannot
/// be diffed.
pub struct OutdatedPackage {
    /// The `package.json` key (and `node_modules` directory name). Equals
    /// `package_name` except for npm-alias entries (`"foo": "npm:bar@^1"`).
    pub alias: String,
    /// The registry package name actually queried.
    pub package_name: String,
    pub belongs_to: DependencyGroup,
    pub current: Version,
    pub target: Version,
    pub wanted: Version,
    pub github_action: bool,
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
            deprecated: None,
            homepage: Some(action.homepage),
            workspace: None,
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
}

/// The matcher for `updateConfig.ignoreDependencies`, or [`None`] when
/// nothing is ignored.
pub(crate) fn ignored_dependencies_matcher(config: &Config) -> Option<Matcher> {
    config
        .update_config
        .ignore_dependencies
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
        &OutdatedRun::new(config, Arc::clone(http_client))?,
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
    let current_versions =
        current_versions_from_importer(lockfile, importer_id, query.include_direct);
    let current_versions = &current_versions;
    // Recorded on every entry so the interactive update list can say
    // which workspace project each outdated dependency came from. A
    // project is not required to declare a name, and an empty label
    // leaves several unnamed projects indistinguishable, so fall back to
    // the path that identifies the project in the lockfile.
    let workspace = manifest
        .value()
        .get("name")
        .and_then(serde_json::Value::as_str)
        // A name that is missing, empty, or only whitespace all give an
        // equally blank label.
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| importer_id.to_string(), str::to_string);
    let workspace = &workspace;

    // Gather the lockfile-pinned direct dependencies to inspect, then
    // fetch their packuments concurrently — mirroring pnpm's
    // `Promise.all` fan-out. Concurrency is bounded by the HTTP client's
    // per-registry limit (`network_concurrency`), so this does not flood
    // the registry. Dependencies without a lockfile pin are dropped here.
    let fetches = query
        .include_direct
        .iter()
        .flat_map(move |&group| {
            manifest.dependencies([group]).filter_map(move |(alias, bare_specifier)| {
                if query.match_names.is_some_and(|matcher| !matcher.matches(alias))
                    || query.ignore_names.is_some_and(|matcher| matcher.matches(alias))
                {
                    return None;
                }
                let current = current_versions.get(alias).cloned()?;
                Some(OutdatedCandidate { alias, group, bare_specifier, current })
            })
        })
        .map(|candidate| outdated_dependency(run, query, workspace, candidate));

    let fetched = futures_util::future::join_all(fetches)
        .await
        .into_iter()
        .collect::<miette::Result<Vec<_>>>()?;
    Ok(fetched.into_iter().flatten().collect())
}

/// One lockfile-pinned direct dependency to compare against the registry.
struct OutdatedCandidate<'a> {
    alias: &'a str,
    group: DependencyGroup,
    bare_specifier: &'a str,
    current: Version,
}

/// The dependency's outdated entry: `None` when the registry names no
/// target, or the target is neither newer nor (when asked) deprecated.
async fn outdated_dependency(
    run: &OutdatedRun,
    query: &OutdatedQuery<'_>,
    workspace: &str,
    candidate: OutdatedCandidate<'_>,
) -> miette::Result<Option<OutdatedPackage>> {
    let bare_specifier =
        dereference_catalog(&run.catalogs, candidate.alias, candidate.bare_specifier)?;
    let resolved_package_name =
        PackageManifest::resolve_registry_dependency(candidate.alias, &bare_specifier)
            .0
            .to_string();
    let latest = resolve_outdated_target(
        run,
        query,
        &candidate,
        bare_specifier.into_owned(),
        &resolved_package_name,
    )
    .await?;
    let Some(target_manifest) = latest.and_then(|latest| latest.latest_manifest) else {
        return Ok(None);
    };
    Ok(outdated_target(query, workspace, candidate, &target_manifest, &resolved_package_name))
}

/// Replace a `catalog:` specifier with the specifier the catalog holds,
/// so both the queried package name and the `--compatible` range come
/// from the catalog entry — which may itself be an npm alias
/// (`npm:@types/table@^6`). Any other specifier passes through.
fn dereference_catalog<'a>(
    catalogs: &Catalogs,
    alias: &str,
    bare_specifier: &'a str,
) -> miette::Result<Cow<'a, str>> {
    // Most dependencies are not catalog entries, and every walked
    // dependency reaches this, so screen them out before the resolver's
    // owned `WantedDependency`.
    if parse_catalog_protocol(bare_specifier).is_none() {
        return Ok(Cow::Borrowed(bare_specifier));
    }
    let wanted = CatalogWantedDependency {
        alias: alias.to_string(),
        bare_specifier: bare_specifier.to_string(),
    };
    match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => Ok(Cow::Owned(found.resolution.specifier)),
        CatalogResolutionResult::Misconfiguration(misconfiguration) => {
            Err(miette::Report::new(misconfiguration.error))
        }
        CatalogResolutionResult::Unused => Ok(Cow::Borrowed(bare_specifier)),
    }
}

pub(super) fn current_versions_from_importer(
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    include_direct: &[DependencyGroup],
) -> HashMap<String, Version> {
    let mut map = HashMap::new();
    let Some(importer) = lockfile.and_then(|lockfile| lockfile.importers.get(importer_id)) else {
        return map;
    };
    for (name, spec) in importer.dependencies_by_groups(include_direct.iter().copied()) {
        if let Some(version) = spec.version.ver_peer().and_then(|ver| ver.version_semver()) {
            map.insert(name.to_string(), version.clone());
        }
    }
    map
}

async fn resolve_outdated_target(
    run: &OutdatedRun,
    query: &OutdatedQuery<'_>,
    candidate: &OutdatedCandidate<'_>,
    bare_specifier: String,
    resolved_package_name: &str,
) -> miette::Result<Option<pnpm_resolving_resolver_base::LatestInfo>> {
    run.resolver
        .resolve_latest(
            &LatestQuery {
                wanted_dependency: ResolverWantedDependency {
                    alias: Some(candidate.alias.to_string()),
                    bare_specifier: Some(bare_specifier),
                    optional: Some(candidate.group == DependencyGroup::Optional),
                    ..ResolverWantedDependency::default()
                },
                compatible: matches!(query.target_version, TargetVersion::WithinRange),
            },
            &run.resolve_options,
        )
        .await
        .map_err(|error| {
            let reason = pnpm_network::redact_url_credentials(&error.to_string());
            miette::miette!(
                code = "ERR_PNPM_OUTDATED_REGISTRY_ERROR",
                r#"Failed to fetch metadata for "{resolved_package_name}": {reason}"#,
            )
        })
}

fn outdated_target(
    query: &OutdatedQuery<'_>,
    workspace: &str,
    candidate: OutdatedCandidate<'_>,
    target_manifest: &pnpm_resolving_resolver_base::SharedDependencyManifest,
    resolved_package_name: &str,
) -> Option<OutdatedPackage> {
    let target = target_manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .and_then(|version| version.parse::<Version>().ok())?;
    let deprecated =
        target_manifest.get("deprecated").and_then(serde_json::Value::as_str).map(str::to_string);
    let is_newer = target > candidate.current;
    if !(is_newer || (query.include_deprecated && deprecated.is_some())) {
        return None;
    }
    let package_name = target_manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(resolved_package_name)
        .to_string();
    Some(OutdatedPackage {
        alias: candidate.alias.to_string(),
        package_name,
        belongs_to: candidate.group,
        wanted: candidate.current.clone(),
        current: candidate.current,
        target,
        github_action: false,
        deprecated,
        homepage: target_manifest
            .get("homepage")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        workspace: Some(workspace.to_string()),
    })
}
