//! Applying fixes: overrides, ignores, and dependency updates.

pub(super) mod update;

pub(crate) use update::{
    AuditFixObserver, VulnerabilityGuard, fix_with_update, format_fix_with_update_output,
};

use super::{
    Arc, AuditAdvisory, AuditError, AuditReport, BTreeMap, Config, ConfigAuditLevel, DateTime,
    DependencyGroup, Deserialize, HashMap, HashSet, IntoDiagnostic, Lockfile, MultiSelect,
    PackageVersionGuard, Range, RangeSpecStyle, Reporter, ResolutionObserver, State, Update, Utc,
    Version, blue, caret_range_for_patched, color_severity, encode_package_name, green,
    normalize_ghsa_id, normalize_registry, parse_packument_timestamp, patched_range_for_style, red,
    redact_url_userinfo, retry_opts_from_config, satisfies_including_prerelease, send_with_retry,
    severity_name, severity_number,
};
use update::advisory_choices;

/// Filter `report`'s advisories down to the set both fix methods and the
/// interactive prompt operate on: severity at or above `audit_level` and
/// not suppressed by `auditConfig.ignoreGhsas`. Mirrors pnpm's
/// `filterAdvisoriesForFix`.
pub(crate) fn filter_advisories_for_fix(
    report: &AuditReport,
    audit_level: ConfigAuditLevel,
    config: &Config,
) -> BTreeMap<String, AuditAdvisory> {
    let ignore_set = config
        .audit_config
        .ignore_ghsas
        .iter()
        .filter_map(|ghsa| {
            let ghsa = normalize_ghsa_id(ghsa);
            (!ghsa.is_empty()).then_some(ghsa)
        })
        .collect::<HashSet<_>>();
    report
        .advisories
        .iter()
        .filter(|(_, advisory)| severity_number(advisory.severity) >= severity_number(audit_level))
        .filter(|(_, advisory)| {
            let ghsa = normalize_ghsa_id(&advisory.github_advisory_id);
            ghsa.is_empty() || !ignore_set.contains(&ghsa)
        })
        .map(|(id, advisory)| (id.clone(), advisory.clone()))
        .collect()
}

/// `auditConfig.ignoreGhsas` entries split by whether their GHSA id still
/// appears in the audit report.
pub(crate) struct PruneIgnoredGhsasResult {
    pub(crate) pruned: Vec<String>,
    pub(crate) retained: Vec<String>,
}

/// Split `ignored_ghsas` into those still present in `report` — normalized
/// to their canonical spelling and deduplicated (`retained`) — and those
/// that aren't, in their original spelling (`pruned`). Mirrors pnpm's
/// `pruneIgnoredGhsas`.
pub(crate) fn prune_ignored_ghsas(
    ignored_ghsas: &[String],
    report: &AuditReport,
) -> PruneIgnoredGhsasResult {
    let advisory_ghsa_ids = report
        .advisories
        .values()
        .filter(|advisory| !advisory.github_advisory_id.is_empty())
        .map(|advisory| normalize_ghsa_id(&advisory.github_advisory_id))
        .collect::<HashSet<_>>();

    let mut retained_seen = HashSet::new();
    let mut retained = Vec::new();
    let mut pruned = Vec::new();
    for ghsa in ignored_ghsas {
        let normalized = normalize_ghsa_id(ghsa);
        if advisory_ghsa_ids.contains(&normalized) {
            if retained_seen.insert(normalized.clone()) {
                retained.push(normalized);
            }
        } else {
            pruned.push(ghsa.clone());
        }
    }
    PruneIgnoredGhsasResult { pruned, retained }
}

/// Build the `name@vulnerable_versions → patched-range` override map from the
/// fixable advisories (those with an inferred patched range), saving each
/// minimum patched version in the style of `range_spec_style`. Keyed by a
/// `BTreeMap` so the output is sorted, mirroring pnpm's `sortDirectKeys`.
pub(crate) fn create_overrides(
    advisories: &BTreeMap<String, AuditAdvisory>,
    range_spec_style: RangeSpecStyle,
) -> BTreeMap<String, String> {
    let mut overrides = BTreeMap::new();
    for advisory in advisories.values() {
        let Some(patched) = advisory.patched_versions.as_deref() else { continue };
        let key = format!("{}@{}", advisory.module_name, advisory.vulnerable_versions);
        overrides.insert(key, patched_range_for_style(patched, range_spec_style));
    }
    overrides
}

/// Write the override-method fixes to `pnpm-workspace.yaml` and return the
/// user-facing summary. Mirrors the override branch of pnpm's audit handler.
/// `publish_infos` reuses the packument data the report validation already
/// fetched so the age-gate check doesn't request it again.
pub(crate) async fn fix_override(
    advisories: &BTreeMap<String, AuditAdvisory>,
    settings_dir: &std::path::Path,
    config: &Config,
    publish_infos: &HashMap<String, Option<PackumentPublishInfo>>,
) -> miette::Result<String> {
    let overrides = create_overrides(
        advisories,
        RangeSpecStyle::from_save_options(config.save_exact, config.save_prefix.as_deref()),
    );
    if overrides.is_empty() {
        return Ok("No fixes were made".to_string());
    }
    let entries = overrides.iter().map(|(key, value)| (key.as_str(), value.as_str()));
    pnpm_workspace_manifest_writer::set_overrides(settings_dir, entries).map_err(|err| {
        miette::Report::new(err).wrap_err("write overrides to pnpm-workspace.yaml")
    })?;
    let json = serde_json::to_string_pretty(&overrides).into_diagnostic()?;
    let mut output = format!(
        "{} overrides were added to pnpm-workspace.yaml to fix vulnerabilities.\nRun \"pnpm install\" to apply the fixes.\n\nThe added overrides:\n{json}",
        overrides.len(),
    );
    if let Some(minimum_release_age) = config.resolved_minimum_release_age() {
        let added =
            resolve_minimum_release_age_excludes(advisories, publish_infos, minimum_release_age)?;
        if !added.is_empty() {
            write_age_excludes(settings_dir, &added)?;
            let note = format!(
                "\n\n{} entries were added to minimumReleaseAgeExclude to allow installing the patched versions:\n{}",
                added.len(),
                added.join("\n"),
            );
            output.push_str(&note);
        }
    }
    Ok(output)
}

/// The packument publish info of one package: the `time` map plus the set of
/// deprecated versions.
#[derive(Debug, Clone)]
pub(crate) struct PackumentPublishInfo {
    /// The packument `time` map: version → raw publish timestamp. Includes
    /// the `created` and `modified` metadata keys alongside version keys.
    pub(crate) time: HashMap<String, String>,
    /// Versions the packument marks as deprecated. Deprecated versions are
    /// excluded from patched-version validation — a deprecated release is
    /// not a viable fix even though it exists on the registry. Parsed rather
    /// than kept as raw keys, because the `time` and `versions` maps may spell
    /// the same release differently (`v1.2.3` vs `1.2.3`).
    pub(crate) deprecated: HashSet<Version>,
}

impl PackumentPublishInfo {
    /// The lowest non-deprecated published version satisfying `range` — the
    /// version an inferred patched range actually resolves to — paired with
    /// its `time` key, which the registry may spell in a non-normalized form
    /// (e.g. `v1.2.3`) that the parsed version drops. `None` when no published
    /// version satisfies the range, whether it was never published, skipped,
    /// yanked, or deprecated.
    ///
    /// Stable releases outrank prereleases regardless of order, so a
    /// `4.18.0-beta.1` published before `4.18.0` is never advertised as the
    /// fix. A prerelease still wins when nothing else satisfies the range.
    pub(crate) fn lowest_non_deprecated_version(&self, range: &Range) -> Option<(&str, Version)> {
        self.time
            .keys()
            .filter(|key| key.as_str() != "created" && key.as_str() != "modified")
            .filter_map(|key| Some((key.as_str(), key.parse::<Version>().ok()?)))
            .filter(|(_, version)| !self.deprecated.contains(version))
            .filter(|(_, version)| satisfies_including_prerelease(version, range))
            .min_by(|(_, a), (_, b)| {
                a.is_prerelease().cmp(&b.is_prerelease()).then_with(|| a.cmp(b))
            })
    }
}

/// The packument publish info of one package, or `None` when the packument
/// could not be fetched or carries no usable `time` field. Ports pnpm's
/// `createPublishTimesFetcher`; `None` must read as "no information", not
/// "old", so a genuinely fresh fix keeps its exclusion.
pub(crate) async fn fetch_publish_times(
    name: &str,
    registry: &str,
    config: &Config,
    http_client: &pnpm_network::ThrottledClient,
) -> Option<PackumentPublishInfo> {
    #[derive(Deserialize)]
    struct PackumentTimes {
        time: Option<HashMap<String, String>>,
        versions: Option<HashMap<String, PackumentVersion>>,
    }

    #[derive(Deserialize)]
    struct PackumentVersion {
        deprecated: Option<String>,
    }

    let registry = normalize_registry(registry);
    let url = format!("{registry}{}", encode_package_name(name));
    // The URL is user-configured and may embed credentials; keep only the
    // redacted form, like the audit request does, so retry diagnostics never
    // print them (auth travels in the header instead).
    let url = redact_url_userinfo(&url);
    let authorization = config.auth_headers.for_url_with_package(&registry, Some(name));
    let retry_opts = retry_opts_from_config(config);
    let (_guard, response) = send_with_retry(http_client, &url, retry_opts, |client| {
        // Full metadata: the abbreviated packument has no `time` field.
        let mut request = client.get(&url).header("accept", "application/json; q=1.0, */*");
        if let Some(value) = &authorization {
            request = request.header("authorization", value);
        }
        request
    })
    .await
    .ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    fn deprecated_versions(versions: HashMap<String, PackumentVersion>) -> HashSet<Version> {
        versions
            .into_iter()
            .filter(|(_, manifest)| manifest.deprecated.is_some())
            .filter_map(|(version, _)| version.parse::<Version>().ok())
            .collect()
    }

    let body = response.json::<PackumentTimes>().await.ok()?;
    let time = body.time?;
    Some(PackumentPublishInfo {
        time,
        deprecated: deprecated_versions(body.versions.unwrap_or_default()),
    })
}

/// Compute the age-gate exclusions for `advisories` using the publish-time
/// maps the report validation already fetched, so a patched version published
/// long before the cutoff gets no pointless `minimumReleaseAgeExclude` entry.
/// Ports the publish-time lookup of pnpm's `createMinimumReleaseAgeExcludes`.
fn resolve_minimum_release_age_excludes(
    advisories: &BTreeMap<String, AuditAdvisory>,
    publish_infos: &HashMap<String, Option<PackumentPublishInfo>>,
    minimum_release_age: u64,
) -> miette::Result<Vec<String>> {
    // On overflow leave the cutoff uncomputable, as `PickPolicy::from_config`
    // does; with no effective gate no bypass entries are needed.
    let Some(cutoff) = i64::try_from(minimum_release_age)
        .ok()
        .and_then(chrono::Duration::try_minutes)
        .and_then(|age| Utc::now().checked_sub_signed(age))
    else {
        return Ok(Vec::new());
    };
    minimum_release_age_excludes(advisories, publish_infos, cutoff)
}

/// The `minimumReleaseAgeExclude` entries needed to keep the age gate from
/// blocking the patched versions: one `name@version` spec per fixable
/// advisory whose fix — the version
/// [`PackumentPublishInfo::lowest_non_deprecated_version`] resolves the
/// patched range to — is younger than `cutoff`. A version published at or
/// before the cutoff doesn't need a bypass, and a version whose publish time
/// is unknown keeps its entry so a genuinely fresh fix stays installable. An
/// advisory the packument offers no fix for gets no entry. Ports pnpm's
/// `createMinimumReleaseAgeExcludes`.
pub(crate) fn minimum_release_age_excludes(
    advisories: &BTreeMap<String, AuditAdvisory>,
    publish_infos: &HashMap<String, Option<PackumentPublishInfo>>,
    cutoff: DateTime<Utc>,
) -> miette::Result<Vec<String>> {
    let specs: Vec<String> = advisories
        .values()
        .filter_map(|advisory| {
            let patched = advisory.patched_versions.as_deref()?;
            let min = patched
                .strip_prefix(">=")
                .and_then(|version| version.trim().parse::<Version>().ok())?;
            let name = advisory.module_name.trim();
            let Some(info) = publish_infos.get(name).and_then(Option::as_ref) else {
                return Some(format!("{name}@{min}"));
            };
            let range = patched.parse::<Range>().ok()?;
            let (key, lowest) = info.lowest_non_deprecated_version(&range)?;
            match info.time.get(key).and_then(|raw| parse_packument_timestamp(raw)) {
                Some(published_at) if published_at <= cutoff => None,
                // A present-but-unparsable timestamp fails open like unknown
                // publish times.
                _ => Some(format!("{name}@{lowest}")),
            }
        })
        .collect();
    pnpm_config::version_policy::merge_package_version_specs(&specs).map_err(miette::Report::new)
}

/// Merge `added` into the project-local `minimumReleaseAgeExclude` and persist
/// the canonical result. Mirrors pnpm's `writeSettings` re-merge of
/// `[...existing, ...added]`.
pub(crate) fn write_age_excludes(
    settings_dir: &std::path::Path,
    added: &[String],
) -> miette::Result<()> {
    pnpm_workspace_manifest_writer::update_workspace_manifest(
        settings_dir,
        &pnpm_workspace_manifest_writer::UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: added,
            ..Default::default()
        },
    )
    .map_err(|err| {
        miette::Report::new(err).wrap_err("write minimumReleaseAgeExclude to pnpm-workspace.yaml")
    })
}

/// Merge the requested ignores into `auditConfig.ignoreGhsas` and persist
/// them, returning the user-facing summary. Mirrors pnpm's `ignore()`:
/// `--ignore-unfixable` adds every advisory with no inferable fix (erroring
/// when one lacks a GHSA id); otherwise the `--ignore` GHSA ids are added.
pub(crate) fn ignore_vulnerabilities(
    report: &AuditReport,
    config: &Config,
    settings_dir: &std::path::Path,
    ignore: &[String],
    ignore_unfixable: bool,
) -> miette::Result<String> {
    let mut ordered: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for ghsa in config.audit_config.ignore_ghsas.iter().map(|ghsa| normalize_ghsa_id(ghsa)) {
        if !ghsa.is_empty() && seen.insert(ghsa.clone()) {
            ordered.push(ghsa);
        }
    }

    let requested = if ignore_unfixable {
        unfixable_ghsa_ids(report)?
    } else {
        ignore.iter().map(|ghsa| normalize_ghsa_id(ghsa)).collect()
    };
    let mut new_ignores: Vec<String> = Vec::new();
    for ghsa in requested {
        if seen.insert(ghsa.clone()) {
            ordered.push(ghsa.clone());
            new_ignores.push(ghsa);
        }
    }

    pnpm_workspace_manifest_writer::set_audit_ignore_ghsas(settings_dir, &ordered).map_err(
        |err| {
            miette::Report::new(err)
                .wrap_err("write auditConfig.ignoreGhsas to pnpm-workspace.yaml")
        },
    )?;

    Ok(ignored_summary(&new_ignores))
}

fn ignored_summary(new_ignores: &[String]) -> String {
    if new_ignores.is_empty() {
        return "No new vulnerabilities were ignored".to_string();
    }
    format!("{} new vulnerabilities were ignored:\n{}", new_ignores.len(), new_ignores.join("\n"))
}

/// The GHSA ids of every advisory with no inferable fix. An advisory
/// that carries no GHSA id cannot be ignored, so it is an error rather
/// than a silent omission.
fn unfixable_ghsa_ids(report: &AuditReport) -> miette::Result<Vec<String>> {
    report
        .advisories
        .values()
        .filter(|advisory| advisory.patched_versions.is_none())
        .map(|advisory| {
            if advisory.github_advisory_id.is_empty() {
                return Err(AuditError::MissingGhsa {
                    id: advisory.id,
                    module_name: advisory.module_name.clone(),
                }
                .into());
            }
            Ok(normalize_ghsa_id(&advisory.github_advisory_id))
        })
        .collect()
}

/// Prompt the user to choose which fixable vulnerabilities to fix and return
/// the chosen subset. `Ok(None)` means "nothing to do" — the prompt was
/// cancelled or no row was selected; an `Err` means the prompt itself failed
/// (e.g. a non-TTY/CI stdin) and must surface rather than read as a clean
/// audit. Ports pnpm's `interactiveAuditFix`, with the flat `dialoguer`
/// multi-select pacquet's `update --interactive` also uses in place of pnpm's
/// severity-grouped table.
pub(crate) fn interactive_select(
    advisories: BTreeMap<String, AuditAdvisory>,
) -> miette::Result<Option<BTreeMap<String, AuditAdvisory>>> {
    let (keys, labels) = advisory_choices(&advisories);

    // Nothing fixable: mirror pnpm returning the report unchanged (the fix
    // method then makes no changes).
    if keys.is_empty() {
        return Ok(Some(advisories));
    }

    // `interact_opt` distinguishes an explicit cancel (Esc/Ctrl-C → `Ok(None)`)
    // from a prompt failure (`Err`). A failure must not be swallowed into a
    // clean audit, so it propagates; a cancel or empty selection is "nothing
    // to do".
    let selected = MultiSelect::new()
        .with_prompt("Choose which vulnerabilities to fix (space to select, enter to confirm)")
        .items(&labels)
        .interact_opt()
        .into_diagnostic()
        .map_err(|err| err.wrap_err("interactive audit selection failed"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    if selected.is_empty() {
        return Ok(None);
    }
    let chosen: HashSet<&String> = selected.iter().map(|&index| &keys[index]).collect();
    Ok(Some(
        advisories
            .into_iter()
            .filter(|(_, advisory)| {
                chosen
                    .contains(&format!("{}@{}", advisory.module_name, advisory.vulnerable_versions))
            })
            .collect(),
    ))
}
