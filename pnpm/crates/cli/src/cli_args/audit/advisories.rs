use super::{
    AuditError, AuditGraph, AuditReport, AuditVulnerabilityCounts, BTreeMap, Config,
    ConfigAuditLevel, DepKind, Edge, EnvLockfile, GraphImporter, HashMap, HashSet, Include,
    Lockfile, PackageKey, PackumentPublishInfo, Range, RawBulkAdvisory, RetryOpts,
    append_snapshot_edges, bulk_response_to_audit_report, empty_snapshots, env_roots,
    fetch_publish_times, importer_roots, lockfile_to_audit_request, normalize_ghsa_id,
    normalize_registry, pick_registry_for_package, redact_url_userinfo, sanitize_response_body,
    send_with_retry,
};

pub(super) async fn audit(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    include: Include,
    config: &Config,
    http_client: &pnpm_network::ThrottledClient,
) -> Result<AuditReport, AuditError> {
    let audit_request = lockfile_to_audit_request(lockfile, env_lockfile, include);
    let registry = normalize_registry(&config.registry);
    let body = serde_json::to_vec(&audit_request.request)
        .expect("audit request is a map of package names to version strings");
    let authorization = config.auth_headers.for_url(&registry);
    let request_url = redact_url_userinfo(&format!("{registry}-/npm/v1/security/advisories/bulk"));
    let (_, response) = send_with_retry(
        http_client,
        &request_url,
        retry_opts_from_config(config),
        |client| bulk_advisories_request(client, &request_url, &body, authorization.as_deref()),
    )
    .await
    .map_err(|source| AuditError::Network {
        url: request_url.clone(),
        source,
    })?;

    let status = response.status().as_u16();
    let raw_body = response
        .text()
        .await
        .map_err(|source| AuditError::Network {
            url: request_url.clone(),
            source,
        })?;
    let advisories = parse_audit_response(status, &raw_body, request_url)?;
    Ok(bulk_response_to_audit_report(
        advisories,
        &audit_request,
        lockfile,
        env_lockfile,
        include,
    ))
}

fn parse_bulk_advisories(
    raw_body: &str,
    url: &str,
) -> Result<BTreeMap<String, Vec<RawBulkAdvisory>>, AuditError> {
    let parsed: serde_json::Value = serde_json::from_str(raw_body)
        .map_err(|source| AuditError::InvalidJson {
            url: url.to_string(),
            reason: source.to_string(),
            body: sanitize_response_body(raw_body),
        })?;
    serde_json::from_value(parsed.clone())
        .map_err(|_| AuditError::UnexpectedBody {
            url: url.to_string(),
            body: sanitize_response_body(&parsed.to_string()),
        })
}

pub(super) fn retry_opts_from_config(config: &Config) -> RetryOpts {
    config.retry_opts()
}

/// Corrects inferred `patched_versions` ranges against the registry: the
/// inference from `vulnerable_versions` is purely syntactic, so the inferred
/// minimum may not be a viable fix — it may never have been published, been
/// skipped, been yanked, or been deprecated. When the inferred range is
/// satisfiable, it is narrowed to the lowest non-deprecated published version
/// (e.g. `>=4.17.24` becomes `>=4.18.1` when 4.17.24 does not exist and
/// 4.18.0 is deprecated). When no published version satisfies it, the range
/// is dropped entirely. A failed packument lookup leaves the range untouched
/// (fail open). Ports pnpm's `correctInferredPatchedVersions`.
///
/// Returns the fetched publish-time maps so the fix flows can reuse them for
/// the age-gate exclusion check instead of re-fetching each packument.
pub(super) async fn correct_inferred_patched_versions(
    report: &mut AuditReport,
    config: &Config,
    http_client: &pnpm_network::ThrottledClient,
) -> HashMap<String, Option<PackumentPublishInfo>> {
    let names: HashSet<&str> = report.advisories
        .values()
        .filter(|advisory| advisory.patched_versions.is_some())
        .map(|advisory| advisory.module_name.trim())
        .collect();
    if names.is_empty() {
        return HashMap::new();
    }
    let registries: HashMap<String, String> = config
        .resolved_registries()
        .into_iter()
        .collect();
    let fetches = names
        .into_iter()
        .map(|name| {
            let registry = pick_registry_for_package(&registries, name, None);
            async move {
                (
                    name.to_string(),
                    fetch_publish_times(name, &registry, config, http_client)
                        .await,
                )
            }
        });
    let publish_infos: HashMap<String, Option<PackumentPublishInfo>> =
        futures_util::future::join_all(fetches).await.into_iter().collect();
    correct_patched_ranges(report, &publish_infos);
    publish_infos
}

impl<'a> AuditGraph<'a> {
    pub(super) fn main(lockfile: &'a Lockfile) -> Self {
        let empty = empty_snapshots();
        let snapshots = lockfile.snapshots.as_ref().unwrap_or(empty);
        let importers = lockfile.importers
            .iter()
            .map(|(id, importer)| GraphImporter {
                path_segment: id.replace('/', "__"),
                roots: importer_roots(importer),
            })
            .collect();
        Self {
            importers,
            snapshots,
        }
    }

    pub(super) fn env(env_lockfile: &'a EnvLockfile) -> Self {
        let importer = env_lockfile.importers.get(EnvLockfile::ROOT_IMPORTER_KEY);
        let mut importers = Vec::new();
        let Some(importer) = importer else {
            return Self {
                importers,
                snapshots: &env_lockfile.snapshots,
            };
        };
        let config_roots = env_roots(&importer.config_dependencies);
        if !config_roots.is_empty() {
            importers.push(GraphImporter {
                path_segment: "configDependencies".to_string(),
                roots: config_roots
                    .into_iter()
                    .map(|edge| (DepKind::Prod, edge))
                    .collect(),
            });
        }
        if let Some(package_manager_dependencies) =
            &importer.package_manager_dependencies
        {
            let package_manager_roots = env_roots(package_manager_dependencies);
            if !package_manager_roots.is_empty() {
                importers.push(GraphImporter {
                    path_segment: "packageManagerDependencies".to_string(),
                    roots: package_manager_roots
                        .into_iter()
                        .map(|edge| (DepKind::Prod, edge))
                        .collect(),
                });
            }
        }

        Self {
            importers,
            snapshots: &env_lockfile.snapshots,
        }
    }

    pub(super) fn children(&self, key: &PackageKey, include_optional_edges: bool) -> Vec<Edge> {
        let Some(snapshot) = self.snapshots.get(key) else {
            return Vec::new();
        };
        let mut children = Vec::new();
        append_snapshot_edges(&mut children, snapshot.dependencies.as_ref());
        if include_optional_edges {
            append_snapshot_edges(&mut children, snapshot.optional_dependencies.as_ref());
        }
        children
    }
}

pub(super) fn filter_ignored_advisories(
    report: &mut AuditReport,
    config: &Config,
) -> AuditVulnerabilityCounts {
    let ignore_set = config.audit_config.ignore_ghsas
        .iter()
        .filter_map(|ghsa| {
            let ghsa_id = normalize_ghsa_id(ghsa);
            (!ghsa_id.is_empty()).then_some(ghsa_id)
        })
        .collect::<HashSet<_>>();
    if ignore_set.is_empty() {
        return AuditVulnerabilityCounts::default();
    }
    let mut ignored = AuditVulnerabilityCounts::default();
    report.advisories.retain(|_, advisory| {
        let ghsa_id = normalize_ghsa_id(&advisory.github_advisory_id);
        if ghsa_id.is_empty() || !ignore_set.contains(&ghsa_id) {
            return true;
        }
        ignored.increment(advisory.severity);
        false
    });
    ignored
}

pub(super) fn parse_audit_level(value: &str) -> Option<ConfigAuditLevel> {
    match value {
        "info" => Some(ConfigAuditLevel::Info),
        "low" => Some(ConfigAuditLevel::Low),
        "moderate" => Some(ConfigAuditLevel::Moderate),
        "high" => Some(ConfigAuditLevel::High),
        "critical" => Some(ConfigAuditLevel::Critical),
        _ => None,
    }
}

pub(super) fn severity_number(level: ConfigAuditLevel) -> u8 {
    match level {
        ConfigAuditLevel::Info => 0,
        ConfigAuditLevel::Low => 1,
        ConfigAuditLevel::Moderate => 2,
        ConfigAuditLevel::High => 3,
        ConfigAuditLevel::Critical => 4,
    }
}

pub(super) fn severity_name(level: ConfigAuditLevel) -> &'static str {
    match level {
        ConfigAuditLevel::Info => "info",
        ConfigAuditLevel::Low => "low",
        ConfigAuditLevel::Moderate => "moderate",
        ConfigAuditLevel::High => "high",
        ConfigAuditLevel::Critical => "critical",
    }
}

fn correct_patched_ranges(
    report: &mut AuditReport,
    publish_infos: &HashMap<String, Option<PackumentPublishInfo>>,
) {
    for advisory in report.advisories.values_mut() {
        let Some(patched) = advisory.patched_versions.as_deref() else {
            continue;
        };
        let Some(Some(info)) = publish_infos.get(advisory.module_name.trim()) else {
            continue;
        };
        let Ok(range) = patched.parse::<Range>() else {
            continue;
        };
        match info.lowest_non_deprecated_version(&range) {
            None => {
                advisory.patched_versions = None;
                advisory.patched_versions_unpublished = Some(true);
            }
            Some((_, lowest)) => {
                advisory.patched_versions = Some(format!(">={lowest}"));
            }
        }
    }
}

fn parse_audit_response(
    status: u16,
    raw_body: &str,
    request_url: String,
) -> Result<BTreeMap<String, Vec<RawBulkAdvisory>>, AuditError> {
    match status {
        200 => parse_bulk_advisories(raw_body, &request_url),
        404 => Err(AuditError::EndpointNotExists {
            url: request_url,
        }),
        _ => Err(AuditError::BadStatus {
            url: request_url,
            status,
            body: sanitize_response_body(raw_body),
        }),
    }
}

fn bulk_advisories_request(
    client: &reqwest::Client,
    request_url: &str,
    body: &[u8],
    authorization: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .post(request_url)
        .header("content-type", "application/json")
        .body(body.to_vec());
    if let Some(value) = authorization {
        request = request.header("authorization", value);
    }
    request
}
