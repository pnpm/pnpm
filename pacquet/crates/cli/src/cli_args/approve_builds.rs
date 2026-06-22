use clap::Args;
use derive_more::{Display, Error};
use dialoguer::{Confirm, MultiSelect};
use miette::{Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_modules_yaml::{Host, write_modules_manifest};
use pacquet_package_manager::allow_build_key_from_ignored_build;
use pacquet_workspace_manifest_writer::set_allow_builds;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

use crate::{State, cli_args::ignored_builds::get_automatically_ignored_builds};

/// `pacquet approve-builds` — approve dependencies for running scripts
/// during installation. Ports pnpm's
/// [`approve-builds`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/commands/src/policy/approveBuilds.ts).
#[derive(Debug, Args)]
pub struct ApproveBuildsArgs {
    /// Packages to approve (`<pkg>`) or deny (`!<pkg>`). With no packages,
    /// the packages awaiting approval are chosen interactively.
    pub packages: Vec<String>,

    /// Approve all pending dependencies without interactive prompts.
    #[clap(long)]
    pub all: bool,

    /// Reserved for parity with pnpm; `approve-builds` is not supported
    /// with global packages.
    #[clap(long)]
    pub global: bool,
}

/// Errors specific to `approve-builds`. Codes match pnpm's
/// `ERR_PNPM_APPROVE_BUILDS_*` set.
#[derive(Debug, Display, Error, Diagnostic)]
enum ApproveBuildsError {
    #[display(r#""approve-builds" is not supported with global packages"#)]
    #[diagnostic(
        code(ERR_PNPM_APPROVE_BUILDS_NOT_SUPPORTED_WITH_GLOBAL),
        help(
            r#"Use --allow-build when installing globally, e.g. "pnpm add -g --allow-build=<pkg> <pkg>". pnpm will also prompt to allow builds interactively during global install."#
        )
    )]
    NotSupportedWithGlobal,

    #[display("Cannot use --all with positional arguments")]
    #[diagnostic(code(ERR_PNPM_APPROVE_BUILDS_ALL_WITH_ARGS))]
    AllWithArgs,

    #[display("The following packages are not awaiting approval: {}", _0.join(", "))]
    #[diagnostic(code(ERR_PNPM_APPROVE_BUILDS_UNKNOWN_PACKAGES))]
    UnknownPackages(#[error(not(source))] Vec<String>),

    #[display("The following packages are both approved and denied: {}", _0.join(", "))]
    #[diagnostic(code(ERR_PNPM_APPROVE_BUILDS_CONTRADICTING_ARGS))]
    ContradictingArgs(#[error(not(source))] Vec<String>),
}

impl ApproveBuildsArgs {
    /// Validate, prompt, write `allowBuilds`, and clear the decided ignored
    /// builds. Returns the rebuild inputs (`Some`) when packages were
    /// approved to build, or `None` when there is nothing to rebuild; the
    /// caller then drives `run_rebuild` with a reporter.
    ///
    /// This stays synchronous so the non-`Send` `config` / `state` closure
    /// references never cross the rebuild's `await`. `config` loads a fresh
    /// `&Config`; it is called before writing settings (to read the current
    /// state) and `state` after, so the rebuild's allow-build policy
    /// reflects the just-written `allowBuilds`. `dir` is the canonicalized
    /// `--dir`, the fallback settings target when no `pnpm-workspace.yaml`
    /// is found.
    pub fn prepare(
        self,
        dir: &Path,
        config: &dyn Fn() -> miette::Result<&'static mut Config>,
        state: &dyn Fn(bool) -> miette::Result<State>,
    ) -> miette::Result<Option<(State, Vec<String>)>> {
        let ApproveBuildsArgs { packages, all, global } = self;

        if global {
            return Err(ApproveBuildsError::NotSupportedWithGlobal.into());
        }
        if all && !packages.is_empty() {
            return Err(ApproveBuildsError::AllWithArgs.into());
        }

        let initial_config: &Config = config()?;
        let scan = get_automatically_ignored_builds(initial_config)?;
        let Some(pending) = scan.names.filter(|names| !names.is_empty()) else {
            println!("There are no packages awaiting approval");
            return Ok(None);
        };

        let (approved, denied) = partition_params(&packages, &pending)?;

        // The packages to build: explicit approvals, every pending package
        // under `--all`, or the interactive selection otherwise.
        let build_packages: Vec<String> = if !packages.is_empty() {
            sort_unique(approved.clone())
        } else if all {
            sort_unique(pending.clone())
        } else {
            let Some(selected) = prompt_for_builds(&pending)? else {
                // The prompt was interrupted (Esc / Ctrl-C); leave
                // everything untouched, matching pnpm's `ExitPromptError`
                // early exit.
                return Ok(None);
            };
            selected
        };

        // The `allowBuilds` entries to write: each decided package mapped to
        // `true` (build) or `false` (skip). In interactive / `--all` mode
        // every pending package is decided, so unselected ones are recorded
        // as `false`.
        let mut decisions: BTreeMap<String, bool> = BTreeMap::new();
        if packages.is_empty() {
            for pkg in &pending {
                decisions.insert(pkg.clone(), build_packages.contains(pkg));
            }
        } else {
            for pkg in &approved {
                decisions.insert(pkg.clone(), true);
            }
            for pkg in &denied {
                decisions.insert(pkg.clone(), false);
            }
        }

        if !all && packages.is_empty() {
            if build_packages.is_empty() {
                println!("All packages were added to allowBuilds with value false.");
            } else if !confirm_builds(&build_packages)? {
                return Ok(None);
            }
        }

        let settings_dir =
            initial_config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        set_allow_builds(
            &settings_dir,
            decisions.iter().map(|(pkg, &value)| (pkg.as_str(), value)),
        )
        .into_diagnostic()?;

        clear_decided_ignored_builds(
            scan.modules_manifest,
            &scan.modules_dir,
            &packages,
            &approved,
            &denied,
        )?;

        if build_packages.is_empty() {
            return Ok(None);
        }
        // Build state from a freshly loaded config so the rebuild's
        // allow-build policy reflects the `allowBuilds` just written.
        Ok(Some((state(true)?, build_packages)))
    }
}

/// Split `params` into approved (`<pkg>`) and denied (`!<pkg>`) names,
/// validating each is awaiting approval and that none is both.
fn partition_params(
    params: &[String],
    automatically_ignored_builds: &[String],
) -> Result<(Vec<String>, Vec<String>), ApproveBuildsError> {
    let mut approved = Vec::new();
    let mut denied = Vec::new();
    let mut unknown = Vec::new();
    for param in params {
        let name = param.strip_prefix('!').unwrap_or(param);
        if !automatically_ignored_builds.iter().any(|build| build == name) {
            unknown.push(name.to_string());
        } else if param.starts_with('!') {
            denied.push(name.to_string());
        } else {
            approved.push(name.to_string());
        }
    }
    if !unknown.is_empty() {
        return Err(ApproveBuildsError::UnknownPackages(unknown));
    }
    let contradictions: Vec<String> =
        approved.iter().filter(|pkg| denied.contains(pkg)).cloned().collect();
    if !contradictions.is_empty() {
        return Err(ApproveBuildsError::ContradictingArgs(contradictions));
    }
    Ok((approved, denied))
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
        Some(indices) => {
            Ok(Some(indices.into_iter().map(|index| choices[index].clone()).collect()))
        }
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
/// Mirrors pnpm's `writeModulesManifest` block in `approveBuilds`.
fn clear_decided_ignored_builds(
    modules_manifest: Option<pacquet_modules_yaml::Modules>,
    modules_dir: &Path,
    params: &[String],
    approved: &[String],
    denied: &[String],
) -> miette::Result<()> {
    let Some(mut modules) = modules_manifest else {
        return Ok(());
    };
    if modules.ignored_builds.is_none() {
        return Ok(());
    }
    if params.is_empty() {
        modules.ignored_builds = None;
    } else {
        let decided: HashSet<&str> =
            approved.iter().chain(denied.iter()).map(String::as_str).collect();
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
fn sort_unique(names: Vec<String>) -> Vec<String> {
    let mut unique: Vec<String> = names.into_iter().collect::<HashSet<_>>().into_iter().collect();
    unique.sort();
    unique
}

#[cfg(test)]
mod tests;
