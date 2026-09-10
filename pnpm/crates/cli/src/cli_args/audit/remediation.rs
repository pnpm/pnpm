use super::{
    Arc, AuditAdvisory, AuditArgs, AuditError, AuditFixObserver, AuditOutcome, AuditReport,
    BTreeMap, Config, FixContext, FixMethod, GuardExhaustionPolicy, PackageVersionGuard,
    PackageVersionGuardDecision, PackageVersionGuardFuture, Reporter, ResolutionObserver,
    ResolvedPackageHint, State, Version, VulnerabilityGuard, filter_advisories_for_fix,
    fix_override, fix_with_update, format_fix_with_update_output, interactive_select,
    print_command_output, prune_ignored_ghsas, sanitize_inline, satisfies_including_prerelease,
};

/// Drop ignored GHSAs that no longer appear in the report, mirroring
/// pnpm's `audit.ignorePrune` handling.
fn prune_ignored_advisories(
    config: &Config,
    report: &AuditReport,
    settings_dir: &std::path::Path,
) -> miette::Result<()> {
    if !config.audit_ignore_prune.unwrap_or(false) || config.audit_config.ignore_ghsas.is_empty() {
        return Ok(());
    }
    let configured_ghsas = &config.audit_config.ignore_ghsas;
    let prune = prune_ignored_ghsas(configured_ghsas, report);
    report_pruned_ghsas(&prune.pruned);
    // Persist even when nothing was removed: `retained` may still differ
    // from the configured list (deduplicated or case-normalized), and the
    // file should always reflect the canonical form.
    if &prune.retained != configured_ghsas {
        pnpm_workspace_manifest_writer::set_audit_ignore_ghsas(settings_dir, &prune.retained)
            .map_err(|err| {
                miette::Report::new(err)
                    .wrap_err("write auditConfig.ignoreGhsas to pnpm-workspace.yaml")
            })?;
    }
    Ok(())
}

/// The pruned ids keep their original spelling from the
/// repository-controlled workspace manifest, so strip control characters
/// before they reach the terminal.
fn report_pruned_ghsas(pruned: &[String]) {
    if pruned.is_empty() {
        return;
    }
    println!(
        "Removed {} unused ignored GHSA{}: {}",
        pruned.len(),
        if pruned.len() == 1 { "" } else { "s" },
        pruned.iter().map(|ghsa| sanitize_inline(ghsa)).collect::<Vec<_>>().join(", "),
    );
}

impl PackageVersionGuard for VulnerabilityGuard {
    fn check<'a>(&'a self, name: &'a str, version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move {
            let rejected = self.ranges_by_name.get(name).is_some_and(|ranges| {
                version.parse::<Version>().is_ok_and(|version| {
                    ranges.iter().any(|range| satisfies_including_prerelease(&version, range))
                })
            });
            Ok(if rejected {
                PackageVersionGuardDecision::Reject {
                    reason: format!("{name}@{version} is vulnerable"),
                }
            } else {
                PackageVersionGuardDecision::Allow
            })
        })
    }

    /// A package whose every in-range version is vulnerable stays on the
    /// version it would have resolved to anyway; `--fix update` then reports
    /// its advisories as remaining instead of failing the whole run.
    fn exhaustion_policy(&self) -> GuardExhaustionPolicy {
        GuardExhaustionPolicy::AcceptRejected
    }
}

impl ResolutionObserver for AuditFixObserver {
    fn on_resolved(&self, _hint: ResolvedPackageHint<'_>) {}

    fn package_version_guard(&self) -> Option<Arc<dyn PackageVersionGuard>> {
        Some(Arc::clone(&self.guard))
    }

    fn minimum_release_age_exclude_override(&self) -> Option<Vec<String>> {
        if self.age_excludes.is_empty() { None } else { Some(self.age_excludes.clone()) }
    }
}

fn append_age_excludes(output: &mut String, age_excludes: &[String]) {
    if !age_excludes.is_empty() {
        use std::fmt::Write as _;
        write!(
                        output,
                        "\n{} entries were added to minimumReleaseAgeExclude to allow installing the patched versions:\n{}\n",
                        age_excludes.len(),
                        age_excludes.join("\n"),
                    )
                    .expect("writing to a string cannot fail");
    }
}

impl AuditArgs {
    /// Apply the chosen fix method to the advisories that survive the
    /// audit-level, ignored-GHSA and interactive filters.
    pub(super) async fn run_fix<Reporter: self::Reporter + 'static>(
        &self,
        fix_method: FixMethod,
        state: &mut State,
        report: &AuditReport,
        context: &FixContext<'_>,
    ) -> miette::Result<AuditOutcome> {
        prune_ignored_advisories(state.config, report, context.settings_dir)?;
        // Pre-filter by audit-level and ignored GHSAs so the interactive
        // prompt and both fix methods see the same advisory set the
        // override path's fixable filter would.
        let filtered = filter_advisories_for_fix(report, context.audit_level, state.config);
        let Some(filtered) = self.select_advisories(filtered)? else {
            return Ok(AuditOutcome::Clean);
        };
        match fix_method {
            FixMethod::Override => {
                let output = fix_override(
                    &filtered,
                    context.settings_dir,
                    state.config,
                    context.publish_infos,
                )
                .await?;
                print_command_output(&output);
                Ok(AuditOutcome::Clean)
            }
            FixMethod::Update => {
                let (fixed, remaining, age_excludes) = fix_with_update::<Reporter>(
                    state,
                    &filtered,
                    context.lockfile_dir,
                    context.settings_dir,
                    context.publish_infos,
                )
                .await?;
                let mut output = format_fix_with_update_output(&fixed, &remaining, &filtered);
                append_age_excludes(&mut output, &age_excludes);
                print_command_output(&output);
                Ok(if remaining.is_empty() {
                    AuditOutcome::Clean
                } else {
                    AuditOutcome::Vulnerable
                })
            }
        }
    }

    /// The advisories a fix flow acts on. `None` when the interactive
    /// prompt was cancelled or selected nothing — there is nothing to fix.
    fn select_advisories(
        &self,
        filtered: BTreeMap<String, AuditAdvisory>,
    ) -> miette::Result<Option<BTreeMap<String, AuditAdvisory>>> {
        if !self.interactive {
            return Ok(Some(filtered));
        }
        interactive_select(filtered)
    }

    /// Resolve the `--fix` flag (and the `--interactive` implies-override
    /// rule) into a [`FixMethod`]. Mirrors pnpm's fix-method dispatch:
    /// `--fix`/`--fix override` → override, `--fix update` → update,
    /// `--interactive` without `--fix` → override, anything else → error.
    pub(super) fn resolve_fix_method(&self) -> miette::Result<Option<FixMethod>> {
        match self.fix.as_deref() {
            Some("override") => Ok(Some(FixMethod::Override)),
            Some("update") => Ok(Some(FixMethod::Update)),
            Some(value) => Err(AuditError::InvalidFixOption { value: value.to_string() }.into()),
            None if self.interactive => Ok(Some(FixMethod::Override)),
            None => Ok(None),
        }
    }
}
