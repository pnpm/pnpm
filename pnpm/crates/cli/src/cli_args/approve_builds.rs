pub(crate) use prompt::disambiguated_labels;
#[cfg(not(target_family = "wasm"))]
pub(crate) use prompt::{confirm, prompt_for_choices};
#[cfg(target_family = "wasm")]
pub(crate) use wasm::{confirm, prompt_for_choices};

mod prompt;
#[cfg(target_family = "wasm")]
mod wasm;

use crate::{
    State,
    cli_args::ignored_builds::{IgnoredBuildsScan, get_automatically_ignored_builds},
    confirm_prompt::PromptAvailability,
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::{Config, WorkspaceSettings};
use pnpm_modules_yaml::{Host, write_modules_manifest};
use pnpm_package_manager::{allow_build_key_from_ignored_build, parse_allow_build_selector};
use pnpm_reporter::{Reporter, emit_global_warning};
use pnpm_workspace_manifest_writer::{PermissionValue, set_permissions};
use prompt::{confirm_builds_prompt, prompt_for_builds};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

/// Approve dependencies for running scripts during installation.
#[derive(Debug, Args)]
pub struct ApproveBuildsArgs {
    /// Packages to approve (`<pkg>`) or deny (`!<pkg>`). With no packages,
    /// the packages awaiting approval are chosen interactively.
    pub packages: Vec<String>,

    /// Approve all pending dependencies without interactive prompts.
    #[clap(long)]
    pub all: bool,

    /// Approve builds for globally installed packages.
    #[clap(short = 'g', long)]
    pub global: bool,
}

/// Errors specific to `approve-builds`. Codes match pnpm's
/// `ERR_PNPM_APPROVE_BUILDS_*` set.
#[derive(Debug, Display, Error, Diagnostic)]
enum ApproveBuildsError {
    #[display("Cannot use --all with positional arguments")]
    #[diagnostic(code(ERR_PNPM_APPROVE_BUILDS_ALL_WITH_ARGS))]
    AllWithArgs,

    #[display(
        "A package name is missing from the arguments. Please specify the package name(s) to approve (`<pkg>`) or deny (`!<pkg>`)."
    )]
    #[diagnostic(code(ERR_PNPM_APPROVE_BUILDS_MISSING_PACKAGE))]
    MissingPackage,

    #[display("The following packages are both approved and denied: {}", _0.join(", "))]
    #[diagnostic(code(ERR_PNPM_APPROVE_BUILDS_CONTRADICTING_ARGS))]
    ContradictingArgs(#[error(not(source))] Vec<String>),

    #[display(
        r#""pnpm approve-builds" without package arguments or --all requires an interactive terminal"#
    )]
    #[diagnostic(
        code(ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE),
        help(
            r#"Run "pnpm approve-builds" in an interactive terminal outside CI, pass --all, or pass the package names to approve (<pkg>) or deny (!<pkg>)."#
        )
    )]
    NonInteractive,
}

pub(crate) struct ApprovalDecision {
    pub(crate) build_packages: Vec<String>,
    decisions: BTreeMap<String, bool>,
    clear_all: bool,
}

impl ApproveBuildsArgs {
    /// Validate, prompt, write `allowBuilds`, and clear the decided ignored
    /// builds. Returns the rebuild inputs (`Some`) when packages were
    /// approved to build, or `None` when there is nothing to rebuild; the
    /// caller then drives `run_rebuild` with a reporter.
    ///
    /// `config` locates the pending approvals and the settings file. The
    /// returned rebuild state is `rebuild_config` plus the `allowBuilds`
    /// this call wrote, anchored at `manifest_path`. `dir` is the
    /// canonicalized `--dir`, the fallback settings target when no
    /// `pnpm-workspace.yaml` is found.
    pub fn prepare<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        rebuild_config: &Config,
        manifest_path: &Path,
    ) -> miette::Result<Option<(State, Vec<String>)>> {
        self.validate()?;
        let scan = get_automatically_ignored_builds(config)?;
        self.prepare_with_scan::<Reporter>(dir, config, rebuild_config, manifest_path, scan)
    }

    pub(crate) fn prepare_with_scan<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        rebuild_config: &Config,
        manifest_path: &Path,
        scan: IgnoredBuildsScan,
    ) -> miette::Result<Option<(State, Vec<String>)>> {
        self.validate()?;
        let pending = scan.names.unwrap_or_default();
        if pending.is_empty() && self.packages.is_empty() {
            pnpm_diagnostics::println!("There are no packages awaiting approval");
            return Ok(None);
        }
        let Some(decision) = self.decide::<Reporter>(&pending, config.ci)? else {
            return Ok(None);
        };

        let settings_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        write_approval_settings(&settings_dir, &decision)?;
        clear_decided_ignored_builds(scan.modules_manifest, &scan.modules_dir, &decision)?;

        // Only a package that was awaiting approval has something to
        // rebuild. A pre-emptive approval names a package that is not
        // installed yet, and rebuilding for it would demand a lockfile the
        // project may not have.
        let build_packages: Vec<String> = decision.build_packages
            .into_iter()
            .filter(|name| pending.contains(name))
            .collect();
        if build_packages.is_empty() {
            return Ok(None);
        }
        let rebuild_state = State::init(
            manifest_path.to_path_buf(),
            config_with_install_approvals(rebuild_config, &settings_dir)?,
            true,
        )
        .wrap_err("initialize the rebuild state")?;
        Ok(Some((rebuild_state, build_packages)))
    }

    pub(crate) fn decide<Reporter: self::Reporter>(
        self,
        pending: &[String],
        ci: bool,
    ) -> miette::Result<Option<ApprovalDecision>> {
        self.decide_with_prompt::<Reporter>(pending, PromptAvailability::detect(ci))
    }

    pub(crate) fn decide_with_prompt<Reporter: self::Reporter>(
        self,
        pending: &[String],
        prompt: PromptAvailability,
    ) -> miette::Result<Option<ApprovalDecision>> {
        self.validate()?;
        let partition = partition_params(&self.packages, pending);
        if !partition.unknown.is_empty() {
            emit_global_warning::<Reporter>(&format!(
                "The following packages are not awaiting approval: {}",
                partition.unknown.join(", "),
            ));
        }
        partition.reject_contradictions()?;
        let Some(build_packages) = self.select_builds(&partition.approved, pending, prompt)? else {
            return Ok(None);
        };

        let decisions = if self.packages.is_empty() {
            pending
                .iter()
                .map(|pkg| (pkg.clone(), build_packages.contains(pkg)))
                .collect()
        } else {
            named_decisions(&partition.approved, &partition.denied)
        };

        // Only the interactive path asks for confirmation: named
        // packages and `--all` are the answer already.
        if !self.all
            && self.packages.is_empty()
            && !confirm_selected_builds(&build_packages, pending)?
        {
            return Ok(None);
        }

        Ok(Some(ApprovalDecision {
            build_packages,
            decisions,
            clear_all: self.packages.is_empty(),
        }))
    }

    fn select_builds(
        &self,
        approved: &[String],
        pending: &[String],
        prompt: PromptAvailability,
    ) -> miette::Result<Option<Vec<String>>> {
        if !self.packages.is_empty() {
            return Ok(Some(sort_unique(approved.to_owned())));
        }
        if self.all {
            return Ok(Some(sort_unique(pending.to_owned())));
        }
        if pending.is_empty() {
            return Ok(None);
        }
        if !prompt.is_available() {
            return Err(ApproveBuildsError::NonInteractive.into());
        }
        prompt_for_builds(pending)
    }

    pub(crate) fn validate(&self) -> miette::Result<()> {
        if self.all && !self.packages.is_empty() {
            return Err(ApproveBuildsError::AllWithArgs.into());
        }
        if self.packages
            .iter()
            .any(|param| parse_allow_build_selector(param).0.is_empty())
        {
            return Err(ApproveBuildsError::MissingPackage.into());
        }
        Ok(())
    }
}

/// The per-package verdicts of a run that named its packages.
fn named_decisions(approved: &[String], denied: &[String]) -> BTreeMap<String, bool> {
    approved
        .iter()
        .map(|pkg| (pkg.clone(), true))
        .chain(
            denied
                .iter()
                .map(|pkg| (pkg.clone(), false)),
        )
        .collect()
}

/// Whether the interactive run may proceed. An empty selection needs no
/// confirmation — it denies every pending package.
fn confirm_selected_builds(build_packages: &[String], pending: &[String]) -> miette::Result<bool> {
    if build_packages.is_empty() {
        pnpm_diagnostics::println!("All packages were added to allowBuilds with value false.");
        return Ok(true);
    }
    confirm(&confirm_builds_prompt(build_packages, pending))
}

pub(crate) fn write_approval_settings(
    settings_dir: &Path,
    decision: &ApprovalDecision,
) -> miette::Result<()> {
    set_permissions(
        settings_dir,
        decision.decisions
            .iter()
            .map(|(pkg, &value)| (pkg.as_str(), PermissionValue::Build(value))),
    )
    .into_diagnostic()
}

/// The names an `approve-builds` argument list decides, split by verdict.
///
/// A name that is not awaiting approval lands in `unknown` as well as in
/// its verdict: pre-emptive decisions are recorded, but a typo is worth a
/// warning because it silently allows or denies a package that will never
/// be installed under that name.
#[derive(Debug, Default)]
pub(crate) struct Partition {
    pub(crate) approved: Vec<String>,
    pub(crate) denied: Vec<String>,
    pub(crate) unknown: Vec<String>,
}

impl Partition {
    /// Fail when a package is both approved and denied.
    pub(crate) fn reject_contradictions(&self) -> miette::Result<()> {
        let contradictions: Vec<String> = self.approved
            .iter()
            .filter(|pkg| self.denied.contains(pkg))
            .cloned()
            .collect();
        if contradictions.is_empty() {
            return Ok(());
        }
        Err(ApproveBuildsError::ContradictingArgs(contradictions).into())
    }
}

/// Split `params` into approved (`<pkg>`) and denied (`!<pkg>`) names,
/// collecting the ones that are not awaiting approval.
pub(crate) fn partition_params(
    params: &[String],
    automatically_ignored_builds: &[String],
) -> Partition {
    let mut partition = Partition::default();
    for param in params {
        let (name, allowed) = parse_allow_build_selector(param);
        if !automatically_ignored_builds
            .iter()
            .any(|build| build == name)
        {
            partition.unknown.push(name.to_string());
        }
        if allowed {
            partition.approved.push(name.to_string());
        } else {
            partition.denied.push(name.to_string());
        }
    }
    partition
}

/// Drop the now-decided entries from `.modules.yaml`'s `ignoredBuilds` so a
/// later `ignored-builds` / install no longer reports them. With positional
/// arguments only the decided (approved + denied) packages are removed,
/// preserving the still-pending ones; otherwise every entry is cleared.
pub(crate) fn clear_decided_ignored_builds(
    modules_manifest: Option<pnpm_modules_yaml::Modules>,
    modules_dir: &Path,
    decision: &ApprovalDecision,
) -> miette::Result<()> {
    let Some(mut modules) = modules_manifest else {
        return Ok(());
    };
    if modules.ignored_builds.is_none() {
        return Ok(());
    }
    if decision.clear_all {
        modules.ignored_builds = None;
    } else {
        let decided: HashSet<&str> = decision.decisions
            .keys()
            .map(String::as_str)
            .collect();
        if let Some(ignored) = modules.ignored_builds.as_mut() {
            ignored.retain(|dep_path| {
                !decided.contains(allow_build_key_from_ignored_build(dep_path.as_str()).as_str())
            });
        }
        if let Some(ignored) = &modules.ignored_builds
            && ignored.is_empty()
        {
            modules.ignored_builds = None;
        }
    }
    write_modules_manifest::<Host>(modules_dir, modules).into_diagnostic()?;
    Ok(())
}

/// Deduplicate and sort `names` by code unit, matching pnpm's
/// `sortUniqueStrings` (a `Set` then `lexCompare`).
fn sort_unique(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests;

pub(crate) fn exclude_disallowed_builds(scan: &mut IgnoredBuildsScan, config: &Config) {
    if let Some(names) = scan.names.as_mut() {
        names.retain(|name| config.allow_builds.get(name) != Some(&false));
    }
}

/// Run the interactive build-approval flow against the just-installed
/// packages. No-op when nothing is awaiting approval, or when stdin is not a
/// TTY / CI is active (unless the test auto-approve env var is set).
pub(crate) async fn prompt_approve_install_builds<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    install_dir: &Path,
    settings_dir: &Path,
) -> miette::Result<()> {
    let mut scan = get_automatically_ignored_builds(config)?;
    exclude_disallowed_builds(&mut scan, config);
    if scan.names.as_ref().is_none_or(Vec::is_empty) {
        return Ok(());
    }
    let auto_approve = std::env::var("PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS").as_deref() == Ok("1");
    if !auto_approve && !PromptAvailability::detect(config.ci).is_available() {
        return Ok(());
    }

    let manifest_path = install_dir.join("package.json");
    let mut settings_config = config.clone();
    settings_config.workspace_dir = Some(settings_dir.to_path_buf());

    let args = ApproveBuildsArgs { packages: Vec::new(), all: auto_approve, global: false };
    if let Some((rebuild_state, build_packages)) = args.prepare_with_scan::<Reporter>(
        settings_dir,
        &settings_config,
        config,
        &manifest_path,
        scan,
    )? {
        let selection = crate::cli_args::rebuild::RebuildSelection {
            names: Some(build_packages),
            projects: Vec::new(),
        };
        crate::cli_args::rebuild::run_rebuild::<Reporter>(&rebuild_state, selection, None).await?;
    }
    Ok(())
}

/// `config` with the approvals `settings_dir`'s `pnpm-workspace.yaml`
/// records laid over it.
pub(crate) fn config_with_install_approvals(
    config: &Config,
    settings_dir: &Path,
) -> miette::Result<&'static Config> {
    let mut cfg = config.clone();
    if let Some((_, settings)) = WorkspaceSettings::find_and_load(settings_dir)
        .map_err(miette::Report::new)
        .wrap_err("load approved install builds")?
    {
        cfg.allow_builds.extend(settings.decided_build_approvals().unwrap_or_default());
        cfg.allow_skills.extend(settings.decided_skill_approvals().unwrap_or_default());
        // The approval run may have just written `skills.dirs`.
        if let Some(dirs) = settings.skills.and_then(|skills| skills.dirs) {
            cfg.skills_dirs = Some(dirs);
        }
    }
    Ok(Config::leak(cfg))
}
