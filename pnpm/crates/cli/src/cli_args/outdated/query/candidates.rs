use super::{
    CatalogAnchor, CatalogResolutionResult, CatalogWantedDependency, Catalogs, Cow,
    DeclaredEngineDep, DependencyGroup, HashMap, LatestQuery, Lockfile, OutdatedMetadata,
    OutdatedPackage, OutdatedQuery, OutdatedRun, PackageManifest, ResolverWantedDependency,
    TargetVersion, Version, dereference_catalog_protocol, min_version_of_spec,
    resolve_from_catalog,
};

pub(super) struct OutdatedCandidate<'a> {
    pub(super) alias: &'a str,
    pub(super) group: DependencyGroup,
    pub(super) bare_specifier: &'a str,
    pub(super) current: Version,
    pub(super) is_engine: bool,
}

pub(super) fn collect_importer_candidates<'a>(
    manifest: &'a PackageManifest,
    current_versions: &HashMap<String, Version>,
    engine_deps: &'a [DeclaredEngineDep],
    query: &OutdatedQuery<'_>,
) -> Vec<OutdatedCandidate<'a>> {
    let mut candidates = Vec::new();
    for &group in query.include_direct {
        for (alias, bare_specifier) in manifest.dependencies([group]) {
            if let Some(candidate) =
                direct_outdated_candidate(alias, group, bare_specifier, current_versions, query)
            {
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
    candidates
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
    current_versions: &HashMap<String, Version>,
    query: &OutdatedQuery<'_>,
) -> Option<OutdatedCandidate<'a>> {
    if !query.matches_name(alias) {
        return None;
    }
    let current = current_versions
        .get(alias)
        .cloned()
        .or_else(|| {
            pnpm_tool_protocol::is_tool_specifier(bare_specifier)
                .then(|| min_version_of_spec(alias, bare_specifier))
                .flatten()
        })?;
    Some(OutdatedCandidate { alias, group, bare_specifier, current, is_engine: false })
}

pub(super) async fn outdated_dependency(
    run: &OutdatedRun,
    query: &OutdatedQuery<'_>,
    workspace: &str,
    candidate: OutdatedCandidate<'_>,
) -> miette::Result<Option<OutdatedPackage>> {
    let bare_specifier =
        dereference_catalog(&run.catalogs, candidate.alias, candidate.bare_specifier)?;
    let resolved_package_name = if candidate.is_engine {
        candidate.alias.to_string()
    } else {
        PackageManifest::resolve_registry_dependency(candidate.alias, &bare_specifier)
            .0
            .to_string()
    };
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

pub(crate) fn current_versions_from_importer(
    lockfile: Option<&Lockfile>,
    importer_id: &str,
    include_direct: &[DependencyGroup],
) -> HashMap<String, Version> {
    let mut map = HashMap::new();
    let Some(importer) = lockfile.and_then(|lockfile| lockfile.importers.get(importer_id)) else {
        return map;
    };
    for (name, spec) in importer.dependencies_by_groups(include_direct.iter().copied()) {
        let Some(ver) = spec.version.ver_peer() else {
            continue;
        };
        if let Some(version) = ver
            .version_semver()
            .or_else(|| ver.registry_qualified().map(|(_, version)| version))
        {
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
    let deprecated = target_manifest
        .get("deprecated")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let is_newer = target > candidate.current;
    if !(is_newer || (query.include_deprecated && deprecated.is_some())) {
        return None;
    }
    let package_name = if candidate.is_engine {
        candidate.alias.to_string()
    } else {
        target_manifest
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(resolved_package_name)
            .to_string()
    };
    Some(OutdatedPackage {
        alias: candidate.alias.to_string(),
        package_name,
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
        },
    })
}
