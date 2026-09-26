//! The verify-deps-before-run gate: before `pnpm run` / `pnpm exec`
//! execute anything, verify that `node_modules` is in sync with the
//! lockfile and apply the configured action — spawn an install, prompt
//! for one, error out, or warn. pnpm's counterpart is
//! `runDepsStatusCheck` in `exec/commands`.

use super::reporter::{ReporterType, quiet_loglevel_arg};
use derive_more::{Display, Error};
use dialoguer::Confirm;
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::{Config, VerifyDepsBeforeRun};
use pnpm_default_reporter::colors::Colors;
use pnpm_fs::DirLock;
use pnpm_package_manager::{RunDepsStatus, check_deps_status_before_run_at, deps_install_root};
use std::{
    collections::HashSet,
    io::{self, IsTerminal},
    path::Path,
    process::{Command, exit},
    time::Duration,
};

/// The per-user lock directory namespace of the gate's install locks.
const INSTALL_LOCK_NAMESPACE: &str = "pnpm-verify-deps-install-locks";

/// How long a gate waits for another gate's install in the same workspace
/// before installing without the lock.
const INSTALL_LOCK_WAIT: Duration = Duration::from_mins(5);

/// Comfortably above how long an install can legitimately take, so a
/// holder that cannot prove it is alive never has its lock stolen.
const INSTALL_LOCK_ABANDONED_AFTER: Duration = Duration::from_mins(30);

#[derive(Debug, Display, Error, Diagnostic)]
enum VerifyDepsError {
    #[diagnostic(code(ERR_PNPM_VERIFY_DEPS_BEFORE_RUN), help(r#"Run "pnpm install""#))]
    OutOfSync { issue: String },

    #[diagnostic(
        code(ERR_PNPM_VERIFY_DEPS_BEFORE_RUN),
        help(
            r#"Run "pnpm install" before running scripts. The "verifyDepsBeforeRun: prompt" setting cannot prompt for confirmation in non-interactive environments."#
        )
    )]
    CannotPrompt { issue: String },
}

/// Run the configured verify-deps-before-run action for the project at
/// `dir`. `Ok(())` means the script may proceed — including after a
/// spawned install, a declined prompt, or a warning.
pub(crate) fn verify_deps_before_run(
    dir: &Path,
    config: &Config,
    reporter: ReporterType,
) -> miette::Result<()> {
    if !config.verify_deps_before_run.is_enabled() {
        return Ok(());
    }
    let Some(status) = check_deps_status_before_run_at(dir, config) else {
        return Ok(());
    };
    let (issue, mut install_args) = match status {
        RunDepsStatus::UpToDate => return Ok(()),
        RunDepsStatus::SkippedPnp => {
            warn(
                matches!(reporter, ReporterType::Silent),
                "verify-deps-before-run does not work with node-linker=pnp",
            );
            return Ok(());
        }
        RunDepsStatus::Outdated { issue, install_args } => (issue, install_args),
    };
    // A filtered `run` or `exec` only selected some of the workspace's
    // projects, so its install has to select the same ones.
    install_args.extend(filter_selector_args(&config.filter, &config.filter_prod));
    match config.verify_deps_before_run {
        VerifyDepsBeforeRun::Install => locked_install(dir, config, &install_args, reporter),
        VerifyDepsBeforeRun::Prompt => prompt_install(dir, config, &install_args, reporter, issue),
        VerifyDepsBeforeRun::Error => Err(VerifyDepsError::OutOfSync { issue }.into()),
        VerifyDepsBeforeRun::Warn => {
            warn(
                matches!(reporter, ReporterType::Silent),
                &format!("Your node_modules are out of sync with your lockfile. {issue}"),
            );
            Ok(())
        }
        // `true` runs the check without acting on the verdict; `false`
        // returned before the check.
        VerifyDepsBeforeRun::True | VerifyDepsBeforeRun::False => Ok(()),
    }
}

/// Run the verify-deps-before-run check before a recursive run or exec.
///
/// When a single shared lockfile covers the entire workspace, verifying the
/// workspace root checks the shared lockfile and shared workspace state once.
///
/// Under dedicated per-project lockfiles (`sharedWorkspaceLockfile: false`),
/// each project owns its own lockfile and workspace state file, and the
/// workspace root may not participate in the install. In that case, each
/// selected project directory is verified independently.
pub(crate) fn verify_deps_before_recursive_run<ProjectPath: AsRef<Path>>(
    workspace_root: &Path,
    selected_project_dirs: impl IntoIterator<Item = ProjectPath>,
    config: &Config,
    reporter: ReporterType,
) -> miette::Result<()> {
    if !config.verify_deps_before_run.is_enabled() {
        return Ok(());
    }
    let mut seen = HashSet::new();
    let project_dirs: Vec<ProjectPath> = selected_project_dirs
        .into_iter()
        .filter(|dir| seen.insert(dir.as_ref().to_path_buf()))
        .collect();
    if project_dirs.is_empty() {
        return Ok(());
    }
    if config.shares_one_lockfile() {
        verify_deps_before_run(workspace_root, config, reporter)
    } else {
        for project_dir in project_dirs {
            verify_deps_before_run(project_dir.as_ref(), config, reporter)?;
        }
        Ok(())
    }
}

/// Install while holding the workspace's gate lock, so concurrent `run` and
/// `exec` gates on one stale tree start one install rather than one each,
/// racing in the same `node_modules`. A gate that found the lock held
/// re-checks the dependencies once it gets the lock, and installs only if
/// its predecessor's install left them out of date.
fn locked_install(
    dir: &Path,
    config: &Config,
    install_args: &[String],
    reporter: ReporterType,
) -> miette::Result<()> {
    let root = deps_install_root(dir, config);
    let (_lock, waited) = acquire_install_lock(&root).unwrap_or_else(|error| {
        warn(
            matches!(reporter, ReporterType::Silent),
            &format!(
                "Could not lock the dependency install at {}: {error}. Installing without it, which is unsafe if another pnpm is installing there concurrently.",
                root.display(),
            ),
        );
        (None, false)
    });
    if !waited {
        return spawn_install(dir, install_args, reporter);
    }
    match check_deps_status_before_run_at(dir, config) {
        Some(RunDepsStatus::Outdated { mut install_args, .. }) => {
            install_args.extend(filter_selector_args(&config.filter, &config.filter_prod));
            spawn_install(dir, &install_args, reporter)
        }
        _ => Ok(()),
    }
}

/// Take the gate's install lock for the workspace at `root`. The flag
/// reports that another process held it, so the dependencies may have
/// been installed meanwhile. The lock is `None` when the wait ran out.
fn acquire_install_lock(root: &Path) -> io::Result<(Option<DirLock>, bool)> {
    let root = pnpm_fs::realpath_missing(&pnpm_fs::lexical_normalize(root))?;
    let path = pnpm_fs::secure_user_lock_file_path(INSTALL_LOCK_NAMESPACE, &root, "lock")?;
    if let Some(lock) =
        DirLock::acquire(path.clone(), Duration::ZERO, INSTALL_LOCK_ABANDONED_AFTER)?
    {
        return Ok((Some(lock), false));
    }
    let lock = DirLock::acquire(path, INSTALL_LOCK_WAIT, INSTALL_LOCK_ABANDONED_AFTER)?;
    Ok((lock, true))
}

/// Re-run the kind of install the workspace state recorded, in-place
/// and with inherited stdio, the way pnpm's `runDepsStatusCheck` spawns
/// `pnpm install` through `runPnpmCli`. Reporter output goes to stderr so
/// the command being run owns stdout, in the format selected by the parent.
/// The spawned install never re-enters this gate: only `run` / `exec` consult
/// it. Its up-to-date shortcuts are bypassed because the pre-run check has
/// already decided that an install is required.
#[expect(clippy::exit, reason = "a failed spawned install must preserve the child exit code")]
fn spawn_install(
    dir: &Path,
    install_args: &[String],
    reporter: ReporterType,
) -> miette::Result<()> {
    let exe = pnpm_executor::current_pnpm_exe().into_diagnostic()?;
    let mut command = Command::new(exe);
    command
        .args(["install", "--verify-deps-before-run-install", "--use-stderr"])
        .args(install_args)
        .current_dir(dir);
    match reporter {
        ReporterType::Default => {}
        ReporterType::AppendOnly => {
            command.arg("--reporter=append-only");
        }
        ReporterType::Ndjson => {
            command.arg("--reporter=ndjson");
        }
        ReporterType::Silent => {
            command.arg("--reporter=silent");
        }
    }
    if let Some(loglevel) = quiet_loglevel_arg() {
        command.arg(loglevel);
    }
    let status = command.status().into_diagnostic()?;
    if !status.success() {
        // The child already reported its own failure; propagate its exit
        // code without a second error dump (`exitCode ?? 1`, like the
        // exec path).
        exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Print a `globalWarn`-shaped line to stderr. The gate runs before any
/// reporter pipeline exists, so it renders the label the way the
/// default reporter would.
fn warn(silent: bool, message: &str) {
    if silent {
        return;
    }
    let colors =
        Colors { enabled: pnpm_default_reporter::colors_enabled(std::io::stderr().is_terminal()) };
    eprintln!("{} {message}", colors.warn_label());
}

#[expect(clippy::exit, reason = "an interrupted prompt exits 1, like pnpm's ExitPromptError")]
fn prompt_install(
    dir: &Path,
    config: &Config,
    install_args: &[String],
    reporter: ReporterType,
    issue: String,
) -> miette::Result<()> {
    if !std::io::stdin().is_terminal() {
        return Err(VerifyDepsError::CannotPrompt { issue }.into());
    }
    let command = std::iter::once("install")
        .chain(install_args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let message = format!(
        "Your \"node_modules\" directory is out of sync with the \"pnpm-lock.yaml\" file. This can lead to issues during scripts execution.\n\nWould you like to run \"pnpm {command}\" to update your \"node_modules\"?",
    );
    match Confirm::new()
        .with_prompt(message)
        .default(true)
        .interact()
    {
        Ok(true) => locked_install(dir, config, install_args, reporter),
        Ok(false) => Ok(()),
        // The prompt was interrupted (Esc / Ctrl-C); exit like
        // pnpm's ExitPromptError handler.
        Err(_) => exit(1),
    }
}

/// The `--filter` / `--filter-prod` arguments that reproduce the gated
/// command's project selection in the install the gate spawns.
fn filter_selector_args(filters: &[String], filter_prod: &[String]) -> Vec<String> {
    let mut args = Vec::with_capacity(filters.len() + filter_prod.len());
    for selector in filters {
        args.push(format!("--filter={selector}"));
    }
    for selector in filter_prod {
        args.push(format!("--filter-prod={selector}"));
    }
    args
}

#[cfg(test)]
mod tests;
