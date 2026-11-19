use super::{
    CatalogAnchor, CatalogResolutionResult, CatalogWantedDependency, Catalogs, Cow,
    DeclaredEngineDep, DependencyGroup, HashMap, LatestQuery, Lockfile, OutdatedMetadata,
    OutdatedPackage, OutdatedQuery, OutdatedRun, PackageManifest, ResolverWantedDependency,
    TargetVersion, TrustChange, Version, dereference_catalog_protocol, min_version_of_spec,
    resolve_from_catalog,
};
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_resolving_npm_resolver::{TrustEvidence, get_manifest_trust_evidence};
use pnpm_resolving_resolver_base::{
    PreferredVersions, VersionSelectorEntry, VersionSelectorType, VersionSelectors,
};
use std::sync::Arc;

pub(super) struct OutdatedCandidate<'a> {
    pub(super) alias: &'a str,
    pub(super) group: DependencyGroup,
    pub(super) bare_specifier: &'a str,
    pub(super) current: Version,
    pub(super) is_engine: bool,
}

pub(super) fn collect_importer_candidates<'a>(
    manifest: &'a PackageManifest,
    current_versions: &HashMap<String, Option<Version>>,
    engine_deps: &'a [DeclaredEngineDep],
    query: &OutdatedQuery<'_>,
    catalogs: &Catalogs,
) -> miette::Result<Vec<OutdatedCandidate<'a>>> {
    let mut candidates = Vec::new();
    for &group in query.include_direct {
        for (alias, bare_specifier) in manifest.dependencies([group]) {
            if let Some(candidate) = direct_outdated_candidate(
                alias,
                group,
                bare_specifier,
                current_versions,
                query,
                catalogs,
            )? {
                candidates.push(candidate);
            }
        }
    }
    let mut seen_engines = std::collections::HashSet::new();
    for dep in engine_deps {
        if let Some(candidate) = engine_outdated_candidate(dep, query, &mut seen_engines) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

fn engine_outdated_candidate<'a>(
    dep: &'a DeclaredEngineDep,
    query: &OutdatedQuery<'_>,
    seen: &mut std::collections::HashSet<(&'a str, &'static str, &'a str)>,
) -> Option<OutdatedCandidate<'a>> {
    let key = (dep.name.as_str(), dep.group.into(), dep.raw_specifier.as_str());
    (query.matches_name(&dep.name) && seen.insert(key)).then_some(OutdatedCandidate {
        alias: &dep.name,
        group: dep.group,
        bare_specifier: &dep.resolver_bare_specifier,
        current: dep.current.clone()?,
        is_engine: true,
    })
}

fn direct_outdated_candidate<'a>(
    alias: &'a str,
    group: DependencyGroup,
    bare_specifier: &'a str,
    current_versions: &HashMap<String, Option<Version>>,
    query: &OutdatedQuery<'_>,
    catalogs: &Catalogs,
) -> miette::Result<Option<OutdatedCandidate<'a>>> {
    if !query.matches_name(alias) {
        return Ok(None);
    }
    let current = match current_versions.get(alias) {
        Some(Some(version)) => Some(version.clone()),
        // A resolution that names no registry version, such as a git or
        // `file:` one, has nothing to compare against.
        Some(None) if !pnpm_tool_protocol::is_tool_specifier(bare_specifier) => None,
        _ => current_version_from_specifier(catalogs, alias, bare_specifier)?,
    };
    Ok(current.map(|current| OutdatedCandidate {
        alias,
        group,
        bare_specifier,
        current,
        is_engine: false,
    }))
}

fn current_version_from_specifier(
    catalogs: &Catalogs,
    alias: &str,
    bare_specifier: &str,
) -> miette::Result<Option<Version>> {
    let resolved_specifier = dereference_catalog(catalogs, alias, bare_specifier)?;
    if resolved_specifier.starts_with("workspace:") {
        return Ok(None);
    }
    let (package_name, specifier) =
        PackageManifest::resolve_registry_dependency(alias, &resolved_specifier);
    Ok(min_version_of_spec(package_name, specifier))
}

pub(super) async fn outdated_dependency(
    run: &OutdatedRun,
    query: &OutdatedQuery<'_>,
    workspace: &str,
    candidate: OutdatedCandidate<'_>,
) -> miette::Result<Option<OutdatedPackage>> {
    if matches!(query.target_version, TargetVersion::Tag(_))
        && (candidate.is_engine || get_version_selector_type(candidate.bare_specifier).is_none())
    {
        // The `--tag` rewrite only rewrites a plain version, range, or tag
        // declaration. Every other one (an engine, a catalog entry, an
        // `npm:` alias, a named-registry or `jsr:` spec) is not offered an
        // update that cannot apply. Querying `<alias>@<tag>` on the default
        // registry would also read the wrong packument for most of them.
        return Ok(None);
    }
    let bare_specifier =
        dereference_catalog(&run.catalogs, candidate.alias, candidate.bare_specifier)?;
    let resolved_package_name = if candidate.is_engine {
        candidate.alias.to_string()
    } else {
        PackageManifest::resolve_registry_dependency(candidate.alias, &bare_specifier)
            .0
            .to_string()
    };
    let bare_specifier = bare_specifier.into_owned();
    let latest = resolve_outdated_target(
        run,
        query,
        &candidate,
        bare_specifier.clone(),
        &resolved_package_name,
    )
    .await?;
    let Some(target_manifest) = latest.and_then(|latest| latest.latest_manifest) else {
        return Ok(None);
    };
    let is_engine = candidate.is_engine;
    let Some(mut package) =
        outdated_target(query, workspace, candidate, &target_manifest, &resolved_package_name)
    else {
        return Ok(None);
    };
    if query.apply_trust_policy && !is_engine {
        package.metadata.trust = Some(TrustChange {
            current: current_trust_evidence(run, &package, bare_specifier).await,
            target: get_manifest_trust_evidence(&target_manifest),
        });
    }
    Ok(Some(package))
}

/// The trust evidence of the version `package` has installed, or `None`
/// when it cannot be read.
///
/// `bare_specifier` is resolved again with the installed version
/// preferred, so the lookup goes to the registry the specifier names and
/// reads the packument the target resolution already fetched. The release
/// age and trust filters are dropped: they decide what to install, and the
/// installed version is already chosen. A pick other than the installed
/// version, such as a dist-tag's, is discarded.
async fn current_trust_evidence(
    run: &OutdatedRun,
    package: &OutdatedPackage,
    bare_specifier: String,
) -> Option<TrustEvidence> {
    let mut options = run.resolve_options.clone();
    options.policy = pnpm_resolving_resolver_base::ResolutionPolicyOptions::default();
    options.version.preferred_versions = Arc::new(PreferredVersions::from([(
        package.package_name.clone(),
        VersionSelectors::from([(
            package.current.to_string(),
            VersionSelectorEntry::Plain(VersionSelectorType::Version),
        )]),
    )]));
    let manifest = run.resolver
        .resolve_latest(
            &LatestQuery {
                wanted_dependency: ResolverWantedDependency {
                    alias: Some(package.alias.clone()),
                    bare_specifier: Some(bare_specifier),
                    optional: Some(package.belongs_to == DependencyGroup::Optional),
                    ..ResolverWantedDependency::default()
                },
                compatible: true,
                current_version: None,
            },
            &options,
        )
        .await
        .ok()??
        .latest_manifest?;
    let picked = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)?
        .parse::<Version>()
        .ok()?;
    (picked == package.current).then(|| get_manifest_trust_evidence(&manifest)).flatten()
}

fn dereference_catalog<'a>(
    catalogs: &Catalogs,
    alias: &str,
    bare_specifier: &'a str,
) -> miette::Result<Cow<'a, str>> {
    if dereference_catalog_protocol(bare_specifier).is_none() {
        return Ok(Cow::Borrowed(bare_specifier));
    }
    let wanted = CatalogWantedDependency {
        alias: alias.to_string(),
        bare_specifier: bare_specifier.to_string(),
    };
    match resolve_from_catalog(catalogs, &wanted, CatalogAnchor::AsWritten) {
        CatalogResolutionResult::Found(found) => Ok(Cow::Owned(found.resolution.specifier)),
        CatalogResolutionResult::Misconfiguration(misconfiguration) => {
            Err(miette::Report::new(misconfiguration.error))
        }
        CatalogResolutionResult::Unused => Ok(Cow::Borrowed(bare_specifier)),
    }
}

/// The version the importer records for each direct dependency in
/// `include_direct`, or `None` when its resolution names no registry version.
pub(crate) fn current_versions_from_importer(
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    include_direct: &[DependencyGroup],
) -> HashMap<String, Option<Version>> {
    let mut map = HashMap::new();
    let Some(importer) = lockfile.and_then(|lockfile| lockfile.importers.get(importer_id)) else {
        return map;
    };
    for (name, spec) in importer.dependencies_by_groups(include_direct.iter().copied()) {
        map.insert(name.to_string(), registry_version(spec));
    }
    map
}

fn registry_version(spec: &pnpm_lockfile::ResolvedDependencySpec) -> Option<Version> {
    let ver = spec.version.ver_peer()?;
    ver.version_semver()
        .or_else(|| ver.registry_qualified().map(|(_, version)| version))
        .cloned()
}

async fn resolve_outdated_target(
    run: &OutdatedRun,
    query: &OutdatedQuery<'_>,
    candidate: &OutdatedCandidate<'_>,
    bare_specifier: String,
    resolved_package_name: &str,
) -> miette::Result<Option<pnpm_resolving_resolver_base::LatestInfo>> {
    // A tag target resolves the tag itself, whatever the declared range
    // admits. A package that publishes no such tag drops out of the list the
    // way one with no compatible target does.
    let (bare_specifier, compatible) = match query.target_version {
        TargetVersion::Tag(tag) => (tag.to_string(), true),
        TargetVersion::Latest => (bare_specifier, false),
        TargetVersion::WithinRange => (bare_specifier, true),
    };
    let result = run.resolver.resolve_latest(
        &LatestQuery {
            wanted_dependency: ResolverWantedDependency {
                alias: Some(candidate.alias.to_string()),
                bare_specifier: Some(bare_specifier),
                optional: Some(candidate.group == DependencyGroup::Optional),
                ..ResolverWantedDependency::default()
            },
            compatible,
            current_version: query.follow_prerelease_channels.then(|| {
                candidate.current.clone()
            }),
        },
        &run.resolve_options,
    )
    .await;
    match result {
        Ok(latest) => Ok(latest),
        Err(error) if tag_query_misses(query, &error) => Ok(None),
        Err(error) => Err({
            let reason = pnpm_network::redact_url_credentials(&error.to_string());
            miette::miette!(
                code = "ERR_PNPM_OUTDATED_REGISTRY_ERROR",
                r#"Failed to fetch metadata for "{resolved_package_name}": {reason}"#,
            )
        }),
    }
}

/// Whether a failed tag-target query means the package has no such tag
/// rather than a registry problem: the resolver found the packument but no
/// version the tag names.
fn tag_query_misses(
    query: &OutdatedQuery<'_>,
    error: &pnpm_resolving_resolver_base::ResolveError,
) -> bool {
    matches!(query.target_version, TargetVersion::Tag(_))
        && error.is::<pnpm_resolving_resolver_base::NoMatchingVersionError>()
}

/// Whether moving to `target` changes the dependency. A tag target may be
/// older than the current version, and a declaration tracking another tag is
/// rewritten to the requested one even when both name the same version.
fn target_moves(
    target_version: TargetVersion<'_>,
    target: &Version,
    current: &Version,
    declared: &str,
) -> bool {
    match target_version {
        TargetVersion::Tag(tag) => {
            target != current
                || (matches!(get_version_selector_type(declared), Some(VersionSelectorType::Tag))
                    && declared != tag)
        }
        TargetVersion::Latest | TargetVersion::WithinRange => target > current,
    }
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
    let deprecated = target_manifest
        .get("deprecated")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    if !(target_moves(query.target_version, &target, &candidate.current, candidate.bare_specifier)
        || (query.include_deprecated && deprecated.is_some()))
    {
        return None;
    }
    Some(OutdatedPackage {
        alias: candidate.alias.to_string(),
        package_name: target_package_name(&candidate, target_manifest, resolved_package_name),
        belongs_to: candidate.group,
        wanted: candidate.current.clone(),
        current: candidate.current,
        target,
        github_action: false,
        metadata: OutdatedMetadata {
            deprecated,
            homepage: target_manifest
                .get("homepage")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            workspace: Some(workspace.to_string()),
            display_versions: None,
            trust: None,
        },
    })
}

fn target_package_name(
    candidate: &OutdatedCandidate<'_>,
    target_manifest: &pnpm_resolving_resolver_base::SharedDependencyManifest,
    resolved_package_name: &str,
) -> String {
    if candidate.is_engine {
        return candidate.alias.to_string();
    }
    target_manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(resolved_package_name)
        .to_string()
}
