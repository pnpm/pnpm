use crate::{State, cli_args::ignored_builds::get_automatically_ignored_builds};
use clap::Args;
use derive_more::{Display, Error};
use dialoguer::{Confirm, MultiSelect};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::{Config, WorkspaceSettings, decided_allow_builds};
use pnpm_modules_yaml::{Host, write_modules_manifest};
use pnpm_package_manager::{allow_build_key_from_ignored_build, parse_allow_build_selector};
use pnpm_reporter::{Reporter, emit_global_warning};
use pnpm_workspace_manifest_writer::set_allow_builds_clearing_legacy;
use std::{
    collections::{BTreeMap, HashSet},
    io::IsTerminal,
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
    /// The rebuild state is built after the settings are written, from
    /// `config` plus the just-written `allowBuilds`, so the rebuild's
    /// allow-build policy reflects the approval. `dir` is the canonicalized
    /// `--dir`, the fallback settings target when no `pnpm-workspace.yaml`
    /// is found; `manifest_path` is the project manifest the rebuild state
    /// is anchored at.
    pub fn prepare<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &'static Config,
        manifest_path: &Path,
    ) -> miette::Result<Option<(State, Vec<String>)>> {
        self.validate()?;
        let scan = get_automatically_ignored_builds(config)?;
        let pending = scan.names.unwrap_or_default();
        if pending.is_empty() && self.packages.is_empty() {
            println!("There are no packages awaiting approval");
            return Ok(None);
        }
        let Some(decision) = self.decide::<Reporter>(&pending)? else {
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
            config_with_install_approvals(config, &settings_dir)?,
            true,
        )
        .wrap_err("initialize the rebuild state")?;
        Ok(Some((rebuild_state, build_packages)))
    }

    pub(crate) fn decide<Reporter: self::Reporter>(
        self,
        pending: &[String],
    ) -> miette::Result<Option<ApprovalDecision>> {
        self.validate()?;
        let ApproveBuildsArgs { packages, all, global: _ } = self;

        let Partition { approved, denied, unknown } = partition_params(&packages, pending);
        if !unknown.is_empty() {
            emit_global_warning::<Reporter>(&format!(
                "The following packages are not awaiting approval: {}",
                unknown.join(", "),
            ));
        }
        let contradictions: Vec<String> = approved
            .iter()
            .filter(|pkg| denied.contains(pkg))
            .cloned()
            .collect();
        if !contradictions.is_empty() {
            return Err(ApproveBuildsError::ContradictingArgs(contradictions).into());
        }
        let build_packages: Vec<String> = if !packages.is_empty() {
            sort_unique(approved.clone())
        } else if all {
            sort_unique(pending.to_owned())
        } else {
            let Some(selected) = prompt_for_builds(pending)? else {
                return Ok(None);
            };
            selected
        };

        let decisions = if packages.is_empty() {
            pending
                .iter()
                .map(|pkg| (pkg.clone(), build_packages.contains(pkg)))
                .collect()
        } else {
            named_decisions(&approved, &denied)
        };

        // Only the interactive path asks for confirmation: named
        // packages and `--all` are the answer already.
        if !all && packages.is_empty() && !confirm_selected_builds(&build_packages)? {
            return Ok(None);
        }

        Ok(Some(ApprovalDecision { build_packages, decisions, clear_all: packages.is_empty() }))
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
fn confirm_selected_builds(build_packages: &[String]) -> miette::Result<bool> {
    if build_packages.is_empty() {
        println!("All packages were added to allowBuilds with value false.");
        return Ok(true);
    }
    confirm_builds(build_packages)
}

pub(crate) fn write_approval_settings(
    settings_dir: &Path,
    decision: &ApprovalDecision,
) -> miette::Result<()> {
    set_allow_builds_clearing_legacy(
        settings_dir,
        decision.decisions
            .iter()
            .map(|(pkg, &value)| (pkg.as_str(), value)),
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
struct Partition {
    approved: Vec<String>,
    denied: Vec<String>,
    unknown: Vec<String>,
}

/// Split `params` into approved (`<pkg>`) and denied (`!<pkg>`) names,
/// collecting the ones that are not awaiting approval.
fn partition_params(params: &[String], automatically_ignored_builds: &[String]) -> Partition {
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

/// Show the checkbox prompt and return the chosen package names, or `None`
/// when the prompt is interrupted.
fn prompt_for_builds(
    automatically_ignored_builds: &[String],
) -> miette::Result<Option<Vec<String>>> {
    let choices = sort_unique(automatically_ignored_builds.to_vec());
    match MultiSelect::new()
        .with_prompt("Choose which packages to build (<space> to select, <enter> to confirm)")
        .items(&choices)
        .interact_opt()
        .into_diagnostic()?
    {
        Some(indices) => Ok(Some(
            indices
                .into_iter()
                .map(|index| choices[index].clone())
                .collect(),
        )),
        None => Ok(None),
    }
}

/// Ask the user to confirm building `build_packages`. Defaults to "no",
/// matching pnpm's `confirm({ default: false })`.
fn confirm_builds(build_packages: &[String]) -> miette::Result<bool> {
    Confirm::new()
        .with_prompt(format!(
            "The next packages will now be built: {}.\nDo you approve?",
            build_packages.join(", "),
        ))
        .default(false)
        .interact()
        .into_diagnostic()
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

/// Run the interactive build-approval flow against the just-installed
/// packages. No-op when nothing is awaiting approval, or when stdin is not a
/// TTY (unless the test auto-approve env var is set).
pub(crate) async fn prompt_approve_install_builds<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    install_dir: &Path,
    settings_dir: &Path,
) -> miette::Result<()> {
    let pending = get_automatically_ignored_builds(config)?.names.filter(|names| !names.is_empty());
    if pending.is_none() {
        return Ok(());
    }
    let auto_approve = std::env::var("PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS").as_deref() == Ok("1");
    if !auto_approve && !std::io::stdin().is_terminal() {
        return Ok(());
    }

    let manifest_path = install_dir.join("package.json");
    let mut settings_config = config.clone();
    settings_config.workspace_dir = Some(settings_dir.to_path_buf());

    let args = ApproveBuildsArgs { packages: Vec::new(), all: auto_approve, global: false };
    if let Some((rebuild_state, build_packages)) =
        args.prepare::<Reporter>(settings_dir, Config::leak(settings_config), &manifest_path)?
    {
        let selection = crate::cli_args::rebuild::RebuildSelection {
            names: Some(build_packages),
            projects: Vec::new(),
        };
        crate::cli_args::rebuild::run_rebuild::<Reporter>(&rebuild_state, selection, None).await?;
    }
    Ok(())
}

fn config_with_install_approvals(
    config: &Config,
    settings_dir: &Path,
) -> miette::Result<&'static Config> {
    let mut cfg = config.clone();
    if let Some((_, settings)) = WorkspaceSettings::find_and_load(settings_dir)
        .map_err(miette::Report::new)
        .wrap_err("load approved install builds")?
        && let Some(allow_builds) = settings.allow_builds
    {
        cfg.allow_builds.extend(decided_allow_builds(allow_builds));
    }
    Ok(Config::leak(cfg))
}
