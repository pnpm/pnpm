pub(crate) use execution::exec_scripts_prepend_node_path;
pub(super) use execution::{RunContext, get_run_script_commands, run_stages};
pub(super) use listing::ScriptSelector;

use super::{
    exec::{ExecArgs, ExecDirs},
    reporter::{ReporterType, reporter_emit},
};
use clap::Args;
use derive_more::{Display, Error};

use execution::{
    ScriptOutcome, no_matching_script, run_selected_scripts, script_concurrency, script_extra_env,
    selected_scripts,
};
use indexmap::IndexMap;

use listing::{render_project_commands, throw_or_filter_hidden_scripts};
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_executor::{
    ProcessTracker, RunScript, ScriptExit, ScriptOutput, ScriptsPrependNodePath, exit_like,
    run_script,
};
use pnpm_injected_deps_syncer::{SyncInjectedDeps, sync_injected_deps};
use pnpm_package_manager::{
    make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
    pnp_path_for_execution,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::{ReadProjectManifestOnlyError, read_project_manifest_only};
use pnpm_workspace_task_scheduler::{ScheduleGraphOptions, TaskCompletion, schedule_graph};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    sync::Mutex,
};

mod recursive;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A pre-defined package script followed by the arguments passed to
    /// it. When empty, the available scripts are listed.
    ///
    /// One positional rather than a script name plus a separate argument
    /// list, so parsing stops *at* the script name — pnpm puts `run` in
    /// `SPECIALLY_ESCAPED_CMDS` to the same effect. Every later token
    /// reaches the script verbatim, including a `--` separator and
    /// anything shaped like a pnpm flag. Splitting the two lets clap keep
    /// parsing past the script name, which swallows both
    /// (pnpm/pnpm#13295). `exec` / `dlx` / `with` take the same shape.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub script: Vec<String>,

    /// Avoid exiting with a non-zero exit code when the script is undefined.
    #[clap(long)]
    pub if_present: bool,

    /// Run the script starting from the given package, skipping every
    /// package that sorts before it. Only meaningful together with the
    /// global `-r` / `--recursive` flag (the `--resume-from` flag).
    #[clap(skip)]
    pub resume_from: Option<String>,

    /// Save the execution result of every package to
    /// `pnpm-exec-summary.json`. Only meaningful together with the
    /// global `-r` / `--recursive` flag (the `--report-summary` flag).
    #[clap(skip)]
    pub report_summary: bool,

    /// Keep running the remaining scripts after one fails instead of
    /// aborting on the first failure (the global `--no-bail` flag).
    /// Applies to a recursive run and to a `/pattern/` run that selects
    /// several scripts; both bail by default.
    #[clap(skip)]
    pub no_bail: bool,

    /// Sort recursive workspace projects topologically before running.
    #[clap(skip = true)]
    pub sort: bool,

    /// Reverse the project order of a recursive run.
    #[clap(skip = true)]
    pub reverse: bool,

    /// Start scripts in all selected projects concurrently.
    #[clap(skip = true)]
    pub parallel: bool,

    /// Run the specified scripts one by one.
    #[clap(long, short = 's')]
    pub sequential: bool,

    /// Print the task graph a recursive run would execute, without
    /// running anything. Only meaningful together with the global `-r` /
    /// `--recursive` flag.
    #[clap(long = "dry-run")]
    pub dry_run: bool,

    /// With `--dry-run`, print the tasks and their resolved dependency
    /// edges as JSON.
    #[clap(long)]
    pub json: bool,
}

/// Errors from `pacquet run`, including the hidden-script rejections from
/// the script filter.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RunError {
    #[diagnostic(transparent)]
    Manifest(#[error(source)] ReadProjectManifestOnlyError),

    #[display("Missing script: {script}")]
    #[diagnostic(code(ERR_PNPM_NO_SCRIPT), help("{hint}"))]
    NoScript { script: String, hint: String },

    #[display("Script \"{script}\" is hidden and cannot be run directly")]
    #[diagnostic(
        code(ERR_PNPM_HIDDEN_SCRIPT),
        help(r#"Scripts starting with "." are hidden and can only be called from other scripts."#)
    )]
    HiddenScript { script: String },

    #[display("All matched scripts are hidden and cannot be run directly: {scripts}")]
    #[diagnostic(
        code(ERR_PNPM_HIDDEN_SCRIPT),
        help(r#"Scripts starting with "." are hidden and can only be called from other scripts."#)
    )]
    AllHidden { scripts: String },

    #[display("Missing script start or file server.js")]
    #[diagnostic(code(ERR_PNPM_NO_SCRIPT_OR_SERVER))]
    NoScriptOrServer,

    #[display("RegExp flags are not supported in script command selector")]
    #[diagnostic(code(ERR_PNPM_UNSUPPORTED_SCRIPT_COMMAND_FORMAT))]
    UnsupportedScriptCommandFormat,

    #[display("Some scripts failed: {failed} of {total}")]
    #[diagnostic(code(ERR_PNPM_RUN_FAILED), help("{hint}"))]
    SomeScriptsFailed { failed: usize, total: usize, hint: String },

    #[display("The --dry-run option is only supported with recursive runs")]
    #[diagnostic(
        code(ERR_PNPM_DRY_RUN_NOT_RECURSIVE),
        help(
            r#"Use "pnpm -r run --dry-run <script>" to print the task graph of a recursive run."#
        )
    )]
    DryRunNotRecursive,
}

impl RunArgs {
    /// Build the positional from a script name and its arguments, for the
    /// paths that synthesize a `run` rather than parsing one.
    pub(super) fn script<Args>(name: &str, args: Args) -> Vec<String>
    where
        Args: IntoIterator<Item = String>,
    {
        std::iter::once(name.to_string()).chain(args).collect()
    }

    /// The script to run, or `None` when `run` was given no positional and
    /// should list the available scripts instead.
    pub(super) fn script_name(&self) -> Option<&str> {
        self.script.first().map(String::as_str)
    }

    /// The arguments to forward to the script, verbatim.
    pub(super) fn script_args(&self) -> &[String] {
        self.script.get(1..).unwrap_or_default()
    }

    /// Execute the subcommand in `dir`. `silent` suppresses the
    /// `$ <script>` echo (set when the reporter is `silent`).
    ///
    /// On a non-zero script exit code this terminates the process with
    /// the same code, matching pnpm where a failing script sets the
    /// process exit code.
    ///
    /// The `resume_from` / `report_summary` fields are only meaningful
    /// for the recursive path (see [`Self::run_recursive`]) and are
    /// ignored here. `no_bail` applies to a `/pattern/` run and, as in
    /// pnpm 11, to a single selected script: every script runs, and the
    /// command ends with [`RunError::SomeScriptsFailed`] if any failed.
    pub fn run(self, dir: &Path, config: &Config, reporter: ReporterType) -> miette::Result<()> {
        self.run_inner(ExecDirs::same(dir), config, reporter, false)
    }

    /// Like [`Self::run`], but a name that matches no script is handed to
    /// `exec`, which runs it in `dirs.run`.
    pub fn run_fallback(
        self,
        dirs: ExecDirs<'_>,
        config: &Config,
        reporter: ReporterType,
    ) -> miette::Result<()> {
        self.run_inner(dirs, config, reporter, true)
    }

    fn run_inner(
        self,
        dirs: ExecDirs<'_>,
        config: &Config,
        reporter: ReporterType,
        fallback_to_exec: bool,
    ) -> miette::Result<()> {
        let dir = dirs.project;
        // Before the dependency verification: an unsupported flag must
        // fail before anything can trigger an install or a prompt.
        if self.dry_run {
            return Err(RunError::DryRunNotRecursive.into());
        }
        // Before the manifest is read, so a mistyped command in a
        // directory without a project skips the check instead of
        // spawning a doomed install (see check_deps_status_before_run_at).
        super::verify_deps::verify_deps_before_run(dir, config, reporter)?;
        let Some((script_name, args)) = self.script.split_first() else {
            let manifest = read_project_manifest_only(dir).map_err(RunError::Manifest)?;
            println!("{}", render_project_commands(manifest.value(), None));
            return Ok(());
        };
        let manifest = match read_project_manifest_only(dir) {
            Ok(manifest) => manifest,
            Err(ReadProjectManifestOnlyError::NoImporterManifestFound { .. })
                if fallback_to_exec =>
            {
                return exec_fallback(script_name, args, dirs, config, reporter);
            }
            Err(err) => return Err(RunError::Manifest(err).into()),
        };

        let specified = selected_scripts(&manifest, script_name)?;
        if specified.is_empty() {
            return no_matching_script(
                script_name,
                args,
                dirs,
                config,
                reporter,
                self.if_present,
                fallback_to_exec,
            );
        }
        self.run_scripts_here(dir, &manifest, config, reporter, specified, args)
    }

    /// Run the selected scripts of the project at `dir`, several at once
    /// when the concurrency allows it.
    fn run_scripts_here(
        &self,
        dir: &Path,
        manifest: &PackageManifest,
        config: &Config,
        reporter: ReporterType,
        specified: Vec<String>,
        args: &[String],
    ) -> miette::Result<()> {
        let extra_env = script_extra_env(config, dir);
        let init_cwd: PathBuf = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
        let concurrency =
            script_concurrency(config, specified.len(), self.parallel, self.sequential);
        // Several scripts running at once share this process's terminal,
        // so their output is prefixed. Their children are tracked only
        // when a failure should cancel the siblings still running, which
        // `--no-bail` rules out.
        let interleaved = specified.len() > 1 && concurrency > 1;
        let bail = !self.no_bail;
        let process_tracker = (interleaved && bail).then(ProcessTracker::foreground);
        let dep_path = dir.to_string_lossy().into_owned();
        let ctx = RunContext {
            manifest,
            dir,
            init_cwd: &init_cwd,
            config,
            extra_env: &extra_env,
            silent: matches!(reporter, ReporterType::Silent),
            output: if interleaved {
                ScriptOutput::Streamed { dep_path: &dep_path, emit: reporter_emit(reporter) }
            } else {
                ScriptOutput::Inherit
            },
            process_tracker: process_tracker.as_ref(),
        };
        let outcome = ScriptOutcome {
            failures: Mutex::new(Vec::new()),
            abort: Mutex::new(None),
            process_tracker: process_tracker.as_ref(),
            bail,
            scripts: specified,
        };
        run_selected_scripts(&ctx, &outcome, args, concurrency)?;
        outcome.into_result()
    }

    /// Execute the subcommand across the `--filter`-selected workspace
    /// projects, in topological order. The recursive counterpart of
    /// [`Self::run`], selected when the global `-r` / `--recursive` flag is set.
    pub fn run_recursive(
        &self,
        config: &Config,
        dir: &Path,
        reporter: ReporterType,
    ) -> miette::Result<()> {
        // A dry run prints what would execute and runs nothing, so it must
        // not let the dependency verification trigger an install either.
        if !self.dry_run {
            super::verify_deps::verify_deps_before_run(dir, config, reporter)?;
        }
        recursive::run_recursive(
            self,
            config,
            dir,
            reporter_emit(reporter),
            matches!(reporter, ReporterType::Ndjson | ReporterType::Silent),
        )
    }
}

fn exec_fallback(
    script_name: &str,
    args: &[String],
    dirs: ExecDirs<'_>,
    config: &Config,
    reporter: ReporterType,
) -> miette::Result<()> {
    ExecArgs {
        command: RunArgs::script(script_name, args.iter().cloned()),
        shell_mode: false,
        resume_from: None,
        report_summary: false,
        no_bail: false,
        sort: true,
        reverse: false,
        parallel: false,
    }
    .run(dirs, config, reporter)
}

#[cfg(test)]
mod tests;

mod execution;

mod listing;
