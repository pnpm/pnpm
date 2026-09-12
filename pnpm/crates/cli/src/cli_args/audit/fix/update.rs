use super::{
    Arc, AuditAdvisory, AuditError, BTreeMap, DependencyGroup, HashMap, HashSet, Lockfile,
    PackageVersionGuard, PackumentPublishInfo, Range, Reporter, ResolutionObserver, State, Update,
    Version, blue, caret_range_for_patched, color_severity, green, red,
    resolve_minimum_release_age_excludes, satisfies_including_prerelease, severity_name,
    severity_number, write_age_excludes,
};

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
    let classification = classify_for_update(advisories);

    // When `minimumReleaseAge` is set, the patched versions are likely
    // fresher than the cutoff; record the ones that actually are as
    // exclusions (persisted to config and injected into this resolve) so the
    // picker may install them.
    let age_excludes = persist_age_excludes(state, advisories, settings_dir, publish_infos)?;

    update_non_vulnerable::<Reporter>(state, &classification, &age_excludes).await?;

    // A missing lockfile here means the update couldn't be verified; mirror
    // pnpm's `fixWithUpdate`, which errors rather than reporting everything
    // fixed against an empty installed set.
    let Some(updated) = Lockfile::load_wanted_from_dir(lockfile_dir)
        .map_err(|err| miette::Report::new(err).wrap_err("re-read the lockfile after update"))?
    else {
        return Err(AuditError::NoLockfileAfterUpdate.into());
    };
    let installed = installed_packages(&updated);
    let (fixed, remaining) = report_fixed_remaining(
        &classification.vulnerabilities,
        &classification.unfixable,
        &classification.unparsable,
        &installed,
    );

    Ok((fixed, remaining, age_excludes))
}

/// The resolver-time guard rejecting every vulnerable range, carrying the
/// age exclusions that let the patched versions through.
fn fix_observer(
    vulnerabilities: &HashMap<String, Vec<(u64, Range)>>,
    age_excludes: Vec<String>,
) -> Arc<dyn ResolutionObserver> {
    let guard_ranges: HashMap<String, Vec<Range>> = vulnerabilities
        .iter()
        .map(|(name, entries)| {
            (name.clone(), entries.iter().map(|(_, range)| range.clone()).collect())
        })
        .collect();
    Arc::new(AuditFixObserver {
        guard: Arc::new(VulnerabilityGuard { ranges_by_name: guard_ranges }),
        age_excludes,
    })
}

/// When `minimumReleaseAge` is set, the patched versions are likely
/// fresher than the cutoff; record the ones that actually are as
/// exclusions (persisted to config and injected into this resolve) so
/// the picker may install them.
fn persist_age_excludes(
    state: &State,
    advisories: &BTreeMap<String, AuditAdvisory>,
    settings_dir: &std::path::Path,
    publish_infos: &HashMap<String, Option<PackumentPublishInfo>>,
) -> miette::Result<Vec<String>> {
    let Some(minimum_release_age) = state.config.resolved_minimum_release_age() else {
        return Ok(Vec::new());
    };
    let added =
        resolve_minimum_release_age_excludes(advisories, publish_infos, minimum_release_age)?;
    if !added.is_empty() {
        write_age_excludes(settings_dir, &added)?;
    }
    Ok(added)
}

/// Every still-installed package name, regardless of how its lockfile
/// key is shaped, plus the subset whose key parses as semver — the only
/// ones a vulnerable range can be checked against.
fn installed_packages(updated: &Lockfile) -> InstalledPackages {
    let mut installed = InstalledPackages { names: HashSet::new(), versions: HashMap::new() };
    for key in updated.snapshots.iter().flatten().map(|(key, _)| key) {
        let name = key.name.to_string();
        installed.names.insert(name.clone());
        if let Some(version) = key.suffix.version_semver() {
            installed.versions.entry(name).or_default().push(version.clone());
        }
    }
    installed
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
        // Still installed, but only via non-semver keys
        // (file:/git/tarball); the range can't be evaluated, so don't
        // claim it's fixed.
        let Some(versions) = installed.versions.get(name) else {
            remaining.extend(entries.iter().map(|(id, _)| *id));
            continue;
        };
        split_by_vulnerability(entries, versions, &mut fixed, &mut remaining);
    }
    let (still_installed, gone): (Vec<_>, Vec<_>) =
        unfixable.iter().partition(|(name, _)| installed.names.contains(*name));
    remaining.extend(still_installed.into_iter().flat_map(|(_, ids)| ids.iter().copied()));
    fixed.extend(gone.into_iter().flat_map(|(_, ids)| ids.iter().copied()));
    // Advisories with an unparsable vulnerable range can't be proven fixed.
    remaining.extend(unparsable.iter().copied());

    (fixed, remaining)
}

/// Sort one package's advisories by whether an installed version still
/// falls in the advisory's vulnerable range.
fn split_by_vulnerability(
    entries: &[(u64, Range)],
    versions: &[Version],
    fixed: &mut Vec<u64>,
    remaining: &mut Vec<u64>,
) {
    for (id, range) in entries {
        let still_vulnerable =
            versions.iter().any(|version| satisfies_including_prerelease(version, range));
        if still_vulnerable {
            remaining.push(*id);
        } else {
            fixed.push(*id);
        }
    }
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
        let Some(advisory) = by_id(id) else {
            return format!("- Advisory with ID {id} (details not found in the audit report)");
        };
        summarize_advisory(advisory, is_fixed)
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

/// One advisory's line in the `--fix update` summary. A fixed advisory
/// reads green whatever its severity; a remaining one keeps its
/// severity's own color.
fn summarize_advisory(advisory: &AuditAdvisory, is_fixed: bool) -> String {
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

pub(super) fn advisory_choices(
    advisories: &BTreeMap<String, AuditAdvisory>,
) -> (Vec<String>, Vec<String>) {
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

    (keys, labels)
}

async fn update_non_vulnerable<Reporter: self::Reporter + 'static>(
    state: &mut State,
    classification: &UpdateClassification,
    age_excludes: &[String],
) -> miette::Result<()> {
    let lockfile_path = state.lockfile_path();
    let lockfile = state
        .lockfile
        .get()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
    Update {
        tarball_mem_cache: Arc::clone(&state.tarball_mem_cache),
        resolved_packages: &state.resolved_packages,
        http_client: &state.http_client,
        http_client_arc: Arc::clone(&state.http_client),
        config: state.config,
        manifest: &mut state.manifest,
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
        supported_architectures: state.config.supported_architectures.clone(),
        lockfile_only: false,
        resolution_observer: Some(fix_observer(
            &classification.vulnerabilities,
            age_excludes.to_vec(),
        )),
    }
    .run::<Reporter>()
    .await
    .map_err(|err| {
        miette::Report::new(err).wrap_err("update dependencies to fix vulnerabilities")
    })?;
    Ok(())
}
