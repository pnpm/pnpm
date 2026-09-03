//! Applying fixes: overrides, ignores, and dependency updates.

use super::{
    Arc, AuditAdvisory, AuditError, AuditReport, BTreeMap, Config, ConfigAuditLevel, DateTime,
    DependencyGroup, Deserialize, HashMap, HashSet, IntoDiagnostic, Lockfile, MultiSelect,
    PackageVersionGuard, Range, RangeSpecStyle, Reporter, ResolutionObserver, State, Update, Utc,
    Version, blue, caret_range_for_patched, color_severity, encode_package_name, green,
    normalize_ghsa_id, normalize_registry, parse_packument_timestamp, patched_range_for_style, red,
    redact_url_userinfo, retry_opts_from_config, satisfies_including_prerelease, send_with_retry,
    severity_name, severity_number,
};

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
    let body = response.json::<PackumentTimes>().await.ok()?;
    let time = body.time?;
    let deprecated = body
        .versions
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, manifest)| manifest.deprecated.is_some())
        .filter_map(|(version, _)| version.parse::<Version>().ok())
        .collect();
    Some(PackumentPublishInfo { time, deprecated })
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
    for ghsa in &config.audit_config.ignore_ghsas {
        let ghsa = normalize_ghsa_id(ghsa);
        if !ghsa.is_empty() && seen.insert(ghsa.clone()) {
            ordered.push(ghsa);
        }
    }

    let mut new_ignores: Vec<String> = Vec::new();
    let mut add = |ghsa: String, ordered: &mut Vec<String>, new_ignores: &mut Vec<String>| {
        if seen.insert(ghsa.clone()) {
            ordered.push(ghsa.clone());
            new_ignores.push(ghsa);
        }
    };

    if ignore_unfixable {
        for advisory in
            report.advisories.values().filter(|advisory| advisory.patched_versions.is_none())
        {
            if advisory.github_advisory_id.is_empty() {
                return Err(AuditError::MissingGhsa {
                    id: advisory.id,
                    module_name: advisory.module_name.clone(),
                }
                .into());
            }
            add(normalize_ghsa_id(&advisory.github_advisory_id), &mut ordered, &mut new_ignores);
        }
    } else {
        for ghsa in ignore {
            add(normalize_ghsa_id(ghsa), &mut ordered, &mut new_ignores);
        }
    }

    pnpm_workspace_manifest_writer::set_audit_ignore_ghsas(settings_dir, &ordered).map_err(
        |err| {
            miette::Report::new(err)
                .wrap_err("write auditConfig.ignoreGhsas to pnpm-workspace.yaml")
        },
    )?;

    if new_ignores.is_empty() {
        Ok("No new vulnerabilities were ignored".to_string())
    } else {
        Ok(format!(
            "{} new vulnerabilities were ignored:\n{}",
            new_ignores.len(),
            new_ignores.join("\n"),
        ))
    }
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
    let mut fixable: Vec<&AuditAdvisory> =
        advisories.values().filter(|advisory| advisory.patched_versions.is_some()).collect();
    fixable.sort_by_key(|advisory| std::cmp::Reverse(severity_number(advisory.severity)));

    let mut keys: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for advisory in fixable {
        let key = format!("{}@{}", advisory.module_name, advisory.vulnerable_versions);
        if !seen.insert(key.clone()) {
            continue;
        }
        let patched =
            advisory.patched_versions.as_deref().map(caret_range_for_patched).unwrap_or_default();
        labels.push(format!(
            "[{}] {} {} ❯ {} {}",
            severity_name(advisory.severity),
            advisory.module_name,
            advisory.vulnerable_versions,
            patched,
            advisory.github_advisory_id,
        ));
        keys.push(key);
    }

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

/// The advisories of a `--fix update` run, partitioned by how the update can
/// act on each: ones with a concrete vulnerable range (guarded against and
/// re-checked after update), ones whose range is `>=0.0.0` / `*` (no version
/// could ever be safe), and ones whose range the registry sent in a form we
/// can't parse.
pub(crate) struct UpdateClassification {
    pub(crate) vulnerabilities: HashMap<String, Vec<(u64, Range)>>,
    pub(crate) unfixable: HashMap<String, Vec<u64>>,
    /// Advisory ids whose `vulnerable_versions` failed to parse. The registry
    /// is untrusted, so a malformed range must not silently drop the advisory
    /// — it is counted as remaining rather than read as a clean exit.
    pub(crate) unparsable: Vec<u64>,
}

pub(crate) fn classify_for_update(
    advisories: &BTreeMap<String, AuditAdvisory>,
) -> UpdateClassification {
    let mut vulnerabilities: HashMap<String, Vec<(u64, Range)>> = HashMap::new();
    let mut unfixable: HashMap<String, Vec<u64>> = HashMap::new();
    let mut unparsable: Vec<u64> = Vec::new();
    for advisory in advisories.values() {
        // The registry is untrusted: trim both the package name and the range
        // so a whitespace-padded name still keys the guard and the
        // installed-name comparison against the (clean) lockfile, and so the
        // sentinel check matches like the rest of the audit range logic
        // (e.g. `infer_patched_versions`).
        let name = advisory.module_name.trim();
        let range = advisory.vulnerable_versions.trim();
        if range == ">=0.0.0" || range == "*" {
            unfixable.entry(name.to_string()).or_default().push(advisory.id);
            continue;
        }
        let Ok(range) = range.parse::<Range>() else {
            unparsable.push(advisory.id);
            continue;
        };
        vulnerabilities.entry(name.to_string()).or_default().push((advisory.id, range));
    }
    UpdateClassification { vulnerabilities, unfixable, unparsable }
}

/// Re-resolve the lockfile to non-vulnerable versions and report which
/// advisories that fixed. Ports pnpm's `fixWithUpdate`: a resolver-time
/// [`PackageVersionGuard`] rejects vulnerable versions so the picker falls
/// back to a safe one, then the post-update lockfile decides fixed vs.
/// remaining. Advisories whose vulnerable range is `>=0.0.0` / `*` cannot be
/// fixed by an update and are remaining iff the package is still installed.
pub(crate) async fn fix_with_update<Reporter: self::Reporter + 'static>(
    state: &mut State,
    advisories: &BTreeMap<String, AuditAdvisory>,
    lockfile_dir: &std::path::Path,
    settings_dir: &std::path::Path,
    publish_infos: &HashMap<String, Option<PackumentPublishInfo>>,
) -> miette::Result<(Vec<u64>, Vec<u64>, Vec<String>)> {
    let UpdateClassification { vulnerabilities, unfixable, unparsable } =
        classify_for_update(advisories);

    // When `minimumReleaseAge` is set, the patched versions are likely
    // fresher than the cutoff; record the ones that actually are as
    // exclusions (persisted to config and injected into this resolve) so the
    // picker may install them.
    let age_excludes = if let Some(minimum_release_age) =
        state.config.resolved_minimum_release_age()
    {
        let added =
            resolve_minimum_release_age_excludes(advisories, publish_infos, minimum_release_age)?;
        if !added.is_empty() {
            write_age_excludes(settings_dir, &added)?;
        }
        added
    } else {
        Vec::new()
    };

    let guard_ranges: HashMap<String, Vec<Range>> = vulnerabilities
        .iter()
        .map(|(name, entries)| {
            (name.clone(), entries.iter().map(|(_, range)| range.clone()).collect())
        })
        .collect();
    let observer: Arc<dyn ResolutionObserver> = Arc::new(AuditFixObserver {
        guard: Arc::new(VulnerabilityGuard { ranges_by_name: guard_ranges }),
        age_excludes: age_excludes.clone(),
    });

    {
        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        Update {
            tarball_mem_cache: Arc::clone(tarball_mem_cache),
            resolved_packages,
            http_client,
            http_client_arc: Arc::clone(http_client),
            config,
            manifest,
            lockfile,
            lockfile_path: Some(&lockfile_path),
            packages: &[],
            latest: false,
            patches: false,
            save_exact: false,
            save: true,
            include_direct: vec![
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            depth: usize::MAX,
            workspace_packages: None,
            supported_architectures: config.supported_architectures.clone(),
            lockfile_only: false,
            resolution_observer: Some(observer),
        }
        .run::<Reporter>()
        .await
        .map_err(|err| {
            miette::Report::new(err).wrap_err("update dependencies to fix vulnerabilities")
        })?;
    }

    // A missing lockfile here means the update couldn't be verified; mirror
    // pnpm's `fixWithUpdate`, which errors rather than reporting everything
    // fixed against an empty installed set.
    let Some(updated) = Lockfile::load_wanted_from_dir(lockfile_dir)
        .map_err(|err| miette::Report::new(err).wrap_err("re-read the lockfile after update"))?
    else {
        return Err(AuditError::NoLockfileAfterUpdate.into());
    };
    // Every still-installed package name, regardless of how its lockfile key
    // is shaped, plus the subset whose key parses as semver (the only ones a
    // vulnerable range can be checked against).
    let mut installed_names: HashSet<String> = HashSet::new();
    let mut installed_versions: HashMap<String, Vec<Version>> = HashMap::new();
    if let Some(snapshots) = updated.snapshots.as_ref() {
        for key in snapshots.keys() {
            let name = key.name.to_string();
            installed_names.insert(name.clone());
            if let Some(version) = key.suffix.version_semver() {
                installed_versions.entry(name).or_default().push(version.clone());
            }
        }
    }

    let installed = InstalledPackages { names: installed_names, versions: installed_versions };
    let (fixed, remaining) =
        report_fixed_remaining(&vulnerabilities, &unfixable, &unparsable, &installed);

    Ok((fixed, remaining, age_excludes))
}

/// The packages present in the post-update lockfile: every name (regardless of
/// lockfile-key shape) plus, for each, the versions whose key parsed as semver.
pub(crate) struct InstalledPackages {
    pub(crate) names: HashSet<String>,
    pub(crate) versions: HashMap<String, Vec<Version>>,
}

/// Decide which advisories an update fixed. An advisory is **fixed** only when
/// its package is gone, or every installed semver version of it escapes the
/// vulnerable range. It stays **remaining** when a vulnerable version is still
/// installed, when the package survives only under non-semver keys (`file:` /
/// git / tarball — unverifiable), when its range is `>=0.0.0` / `*` and the
/// package is still installed, or when its range was unparsable. The
/// conservative bias keeps `audit --fix update` from reporting a clean state
/// it can't prove.
pub(crate) fn report_fixed_remaining(
    vulnerabilities: &HashMap<String, Vec<(u64, Range)>>,
    unfixable: &HashMap<String, Vec<u64>>,
    unparsable: &[u64],
    installed: &InstalledPackages,
) -> (Vec<u64>, Vec<u64>) {
    let mut fixed: Vec<u64> = Vec::new();
    let mut remaining: Vec<u64> = Vec::new();
    for (name, entries) in vulnerabilities {
        if !installed.names.contains(name) {
            fixed.extend(entries.iter().map(|(id, _)| *id));
            continue;
        }
        match installed.versions.get(name) {
            // Still installed, but only via non-semver keys (file:/git/tarball);
            // the range can't be evaluated, so don't claim it's fixed.
            None => remaining.extend(entries.iter().map(|(id, _)| *id)),
            Some(versions) => {
                for (id, range) in entries {
                    let still_vulnerable = versions
                        .iter()
                        .any(|version| satisfies_including_prerelease(version, range));
                    if still_vulnerable {
                        remaining.push(*id);
                    } else {
                        fixed.push(*id);
                    }
                }
            }
        }
    }
    for (name, ids) in unfixable {
        if installed.names.contains(name) {
            remaining.extend(ids.iter().copied());
        } else {
            fixed.extend(ids.iter().copied());
        }
    }
    // Advisories with an unparsable vulnerable range can't be proven fixed.
    remaining.extend(unparsable.iter().copied());

    (fixed, remaining)
}

/// Render the `--fix update` summary, mirroring pnpm's
/// `formatFixWithUpdateOutput`: a one-line count, then the fixed and
/// remaining advisories listed severity-high-to-low.
pub(crate) fn format_fix_with_update_output(
    fixed: &[u64],
    remaining: &[u64],
    advisories: &BTreeMap<String, AuditAdvisory>,
) -> String {
    let by_id = |id: u64| advisories.get(&id.to_string());
    let sort_by_severity = |ids: &[u64]| -> Vec<u64> {
        let mut ids = ids.to_vec();
        ids.sort_by_key(|id| {
            std::cmp::Reverse(
                by_id(*id).map_or(-1, |advisory| i32::from(severity_number(advisory.severity))),
            )
        });
        ids
    };
    let fixed = sort_by_severity(fixed);
    let remaining = sort_by_severity(remaining);

    let fixed_word =
        if fixed.len() == 1 { "vulnerability was fixed" } else { "vulnerabilities were fixed" };
    let remaining_word =
        if remaining.len() == 1 { "vulnerability remains" } else { "vulnerabilities remain" };

    let mut lines = vec![format!(
        "{} {fixed_word}, {} {remaining_word}.",
        green(&fixed.len().to_string()),
        red(&remaining.len().to_string()),
    )];

    let summarize = |is_fixed: bool, id: u64| -> String {
        match by_id(id) {
            Some(advisory) => {
                let (severity, title) = if is_fixed {
                    (green(severity_name(advisory.severity)), green(&advisory.title))
                } else {
                    (
                        color_severity(advisory.severity, severity_name(advisory.severity)),
                        color_severity(advisory.severity, &advisory.title),
                    )
                };
                format!(r#"- ({severity}) "{title}" {}"#, blue(&advisory.module_name))
            }
            None => format!("- Advisory with ID {id} (details not found in the audit report)"),
        }
    };

    if !fixed.is_empty() {
        lines.push("\nThe fixed vulnerabilities are:".to_string());
        lines.extend(fixed.iter().map(|id| summarize(true, *id)));
    }
    if !remaining.is_empty() {
        lines.push("\nThe remaining vulnerabilities are:".to_string());
        lines.extend(remaining.iter().map(|id| summarize(false, *id)));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Resolver-time guard that rejects concrete versions matching any known
/// vulnerable range for a package, so `audit --fix update` re-picks a safe
/// version. Ports the `isVulnerable` half of pnpm's
/// `PackageVulnerabilityAudit`.
#[derive(Debug)]
pub(crate) struct VulnerabilityGuard {
    pub(crate) ranges_by_name: HashMap<String, Vec<Range>>,
}

/// Carries the [`VulnerabilityGuard`] and the patched-version
/// `minimumReleaseAgeExclude` entries into the install's resolve pass. The
/// resolution stream itself is not observed (`on_resolved` is a no-op); the
/// observer exists only as the seam the resolver reads both from.
pub(crate) struct AuditFixObserver {
    pub(crate) guard: Arc<dyn PackageVersionGuard>,
    pub(crate) age_excludes: Vec<String>,
}
