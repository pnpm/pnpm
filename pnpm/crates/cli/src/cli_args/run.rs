use super::{
    exec::ExecArgs,
    reporter::{ReporterType, reporter_emit},
};
use clap::Args;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_executor::{
    ProcessTracker, RunScript, ScriptExit, ScriptOutput, ScriptsPrependNodePath, run_script,
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
    fmt::Write as _,
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

    /// Keep running the remaining packages after a script fails instead
    /// of aborting on the first failure. Only meaningful together with
    /// the global `-r` / `--recursive` flag (the `--no-bail` flag;
    /// recursive runs bail by default).
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
    /// The `resume_from` / `report_summary` / `no_bail` fields are only
    /// meaningful for the recursive path (see [`Self::run_recursive`])
    /// and are ignored here.
    pub fn run(self, dir: &Path, config: &Config, reporter: ReporterType) -> miette::Result<()> {
        self.run_inner(dir, config, reporter, false)
    }

    pub fn run_fallback(
        self,
        dir: &Path,
        config: &Config,
        reporter: ReporterType,
    ) -> miette::Result<()> {
        self.run_inner(dir, config, reporter, true)
    }

    fn run_inner(
        self,
        dir: &Path,
        config: &Config,
        reporter: ReporterType,
        fallback_to_exec: bool,
    ) -> miette::Result<()> {
        // Before the dependency verification: an unsupported flag must
        // fail before anything can trigger an install or a prompt.
        if self.dry_run {
            return Err(RunError::DryRunNotRecursive.into());
        }
        // Before the manifest is read, so a mistyped command in a
        // directory without a project skips the check instead of
        // spawning a doomed install (see check_deps_status_before_run_at).
        super::verify_deps::verify_deps_before_run(dir, config, reporter)?;
        let silent = matches!(reporter, ReporterType::Silent);
        let parallel = self.parallel;
        let sequential = self.sequential;
        let RunArgs { script, if_present, .. } = self;
        let Some((script_name, args)) = script.split_first() else {
            let manifest = read_project_manifest_only(dir).map_err(RunError::Manifest)?;
            println!("{}", render_project_commands(manifest.value(), None));
            return Ok(());
        };
        let manifest = match read_project_manifest_only(dir) {
            Ok(manifest) => manifest,
            Err(ReadProjectManifestOnlyError::NoImporterManifestFound { .. })
                if fallback_to_exec =>
            {
                return exec_fallback(script_name, args, dir, config, reporter);
            }
            Err(err) => return Err(RunError::Manifest(err).into()),
        };

        let mut specified = ScriptSelector::new(script_name)?.select_with_start(manifest.value());

        // Hidden scripts (names starting with `.`) can only be invoked
        // from within another script, detected by an inherited
        // `npm_lifecycle_event`.
        if env::var_os("npm_lifecycle_event").is_none() {
            specified = throw_or_filter_hidden_scripts(specified, script_name)?;
        }

        if specified.is_empty() {
            if if_present {
                return Ok(());
            }
            if fallback_to_exec {
                return exec_fallback(script_name, args, dir, config, reporter);
            }
            return Err(RunError::NoScript {
                script: script_name.clone(),
                hint: format!(r#"Command "{script_name}" not found."#),
            }
            .into());
        }

        let mut extra_env = config.extra_env_with_node_options();
        if let Some(pnp_path) = pnp_path_for_execution(config, dir) {
            let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
            extra_env.insert(
                "NODE_OPTIONS".to_string(),
                make_node_require_option(&pnp_path, node_options),
            );
        }
        if let Some(package_map_path) = package_map_path_for_execution(config, dir) {
            let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
            extra_env.insert(
                "NODE_OPTIONS".to_string(),
                make_node_package_map_option(&package_map_path, node_options),
            );
        }

        let init_cwd: PathBuf = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
        let concurrency = if parallel {
            specified.len()
        } else if sequential {
            1
        } else {
            usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1)
        };
        let process_tracker =
            (specified.len() > 1 && concurrency > 1).then(ProcessTracker::foreground);
        let dep_path = dir.to_string_lossy().into_owned();
        let ctx = RunContext {
            manifest: &manifest,
            dir,
            init_cwd: &init_cwd,
            config,
            extra_env: &extra_env,
            silent,
            output: if specified.len() > 1 && concurrency > 1 {
                ScriptOutput::Streamed { dep_path: &dep_path, emit: reporter_emit(reporter) }
            } else {
                ScriptOutput::Inherit
            },
            process_tracker: process_tracker.as_ref(),
        };
        let tasks: IndexMap<String, Vec<String>> =
            specified.into_iter().map(|name| (name, Vec::new())).collect();
        let failure = Mutex::new(None);
        let abort = Mutex::new(None);
        let on_script_skipped = |_: &String| {};
        let run_script = |name: String| {
            // Resolve the main body (with `start` → `node server.js`
            // fallback) and apply the args-aware `npx only-allow pnpm`
            // no-op skip. After both pass, [`run_stages`] is
            // guaranteed to actually run the main stage, so its return
            // is a plain [`ScriptExit`].
            let main = match resolve_main_script(&ctx, &name) {
                Ok(Some(main)) => main,
                Ok(None) => return TaskCompletion::Passed,
                Err(error) => {
                    let mut abort = abort.lock().expect("run abort lock is not poisoned");
                    if abort.is_none() {
                        *abort = Some(miette::Report::new(error));
                    }
                    if let Some(process_tracker) = &process_tracker {
                        process_tracker.cancel();
                    }
                    return TaskCompletion::Aborted;
                }
            };
            if args.is_empty() && main == "npx only-allow pnpm" {
                return TaskCompletion::Passed;
            }
            match run_stages(&ctx, &name, &main, args) {
                Ok(status) if status.success() => TaskCompletion::Passed,
                Ok(status) => {
                    let mut failure = failure.lock().expect("run failure lock is not poisoned");
                    if failure.is_none() {
                        *failure = Some(status.code().unwrap_or(1));
                    }
                    if let Some(process_tracker) = &process_tracker {
                        process_tracker.cancel();
                    }
                    TaskCompletion::Failed
                }
                Err(error) => {
                    let mut abort = abort.lock().expect("run abort lock is not poisoned");
                    if abort.is_none() {
                        *abort = Some(error);
                    }
                    if let Some(process_tracker) = &process_tracker {
                        process_tracker.cancel();
                    }
                    TaskCompletion::Aborted
                }
            }
        };
        if concurrency == 1 || tasks.len() == 1 {
            for name in tasks.keys() {
                if !matches!(run_script(name.clone()), TaskCompletion::Passed) {
                    break;
                }
            }
        } else {
            schedule_graph(
                &tasks,
                &ScheduleGraphOptions::new(concurrency, true, &run_script, &on_script_skipped),
            )
            .into_diagnostic()?;
        }
        if let Some(error) = abort.into_inner().expect("run abort lock is not poisoned") {
            return Err(error);
        }
        if let Some(code) = failure.into_inner().expect("run failure lock is not poisoned") {
            // `run_stage` already emitted the `[ELIFECYCLE]` line.
            std::process::exit(code);
        }
        Ok(())
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
    dir: &Path,
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
    .run(dir, config, reporter)
}

/// Shared inputs for running a script, threaded through
/// [`run_stages`] and [`run_stage`] so neither grows an unwieldy
/// argument list. The submodule `recursive` builds a per-project
/// [`RunContext`] and reuses [`run_stages`], so the type and its
/// fields are visible up to the parent module.
pub(super) struct RunContext<'a> {
    pub(super) manifest: &'a PackageManifest,
    pub(super) dir: &'a Path,
    pub(super) init_cwd: &'a Path,
    pub(super) config: &'a Config,
    pub(super) extra_env: &'a HashMap<String, String>,
    pub(super) silent: bool,
    pub(super) output: ScriptOutput<'a>,
    pub(super) process_tracker: Option<&'a ProcessTracker>,
}

/// Resolve `name` to a runnable main script body, or `Ok(None)` when
/// there's nothing to run (the manifest has no truthy `scripts[name]`
/// and `name` isn't `start`). An absent (or empty) `start` falls back
/// to `node server.js` provided `server.js` exists in the script
/// execution directory; otherwise [`RunError::NoScriptOrServer`].
fn resolve_main_script(ctx: &RunContext<'_>, name: &str) -> Result<Option<String>, RunError> {
    let get_script = |key: &str| -> Option<String> {
        ctx.manifest
            .value()
            .get("scripts")
            .and_then(|scripts| scripts.as_object())
            .and_then(|scripts| scripts.get(key))
            .and_then(|script| script.as_str())
            .map(str::to_string)
    };
    match get_script(name) {
        Some(body) if !body.is_empty() => Ok(Some(body)),
        _ if name == "start" => {
            if !ctx.dir.join("server.js").exists() {
                return Err(RunError::NoScriptOrServer);
            }
            Ok(Some("node server.js".to_string()))
        }
        _ => Ok(None),
    }
}

/// Run pre / main / post for `name` around an already-resolved
/// `main_body`. The contract:
///
/// - `main_body` is non-empty.
/// - `main_body` is not `"npx only-allow pnpm"` when `args` is empty
///   (otherwise the main stage's [`run_stage`] would no-op).
///
/// Both callers — single-project [`RunArgs::run`] and the recursive
/// runner — validate these conditions before calling: single-project
/// via [`resolve_main_script`] plus an inline npx-only-allow skip,
/// recursive via its outer per-project filter. Given that, the main
/// stage is guaranteed to actually run, so this function returns a
/// plain [`ScriptExit`] instead of `Option<ScriptExit>` and the callers
/// don't need to defensively handle a "nothing ran" case.
///
/// On the first non-success stage (pre / main / post) the function
/// short-circuits and returns that stage's status; the caller decides
/// what to do with the failure (single-project: `process::exit`;
/// recursive: record `Failure` and bail or continue). A failing stage
/// skips the remaining stages.
///
/// For `run start` with no `start` script but a `prestart`/`poststart`
/// and `enablePrePostScripts`, the hooks run around the `node server.js`
/// fallback, so the `pre`/`post` substring guard runs against the
/// resolved `main_body` here.
pub(super) fn run_stages(
    ctx: &RunContext<'_>,
    name: &str,
    main_body: &str,
    args: &[String],
) -> miette::Result<ScriptExit> {
    let mut main_status = None;
    for (stage, script) in
        get_run_script_stages(ctx.manifest, name, main_body, ctx.config.enable_pre_post_scripts)
    {
        let is_main = stage == name;
        if let Some(status) = run_stage(ctx, &stage, &script, if is_main { args } else { &[] })? {
            if !status.success() {
                return Ok(status);
            }
            if is_main {
                main_status = Some(status);
            }
        }
    }
    let main_status = main_status.expect(
        "caller validated main_body is neither empty nor the args-less `npx only-allow pnpm` no-op",
    );

    if ctx.config.sync_injected_deps_after_scripts.iter().any(|script| script == name) {
        sync_injected_deps(&SyncInjectedDeps {
            pkg_name: ctx.manifest.value().get("name").and_then(Value::as_str),
            pkg_root_dir: ctx.dir,
            workspace_dir: ctx.config.workspace_dir.as_deref(),
            // Read before the script ran, so a bin it drops can still be named.
            manifest_before_scripts: Some(ctx.manifest.value()),
        })?;
    }

    Ok(main_status)
}

pub(super) fn get_run_script_commands(
    manifest: &PackageManifest,
    name: &str,
    main_body: &str,
    enable_pre_post_scripts: bool,
) -> Vec<String> {
    get_run_script_stages(manifest, name, main_body, enable_pre_post_scripts)
        .into_iter()
        .map(|(_, script)| script)
        .collect()
}

fn get_run_script_stages(
    manifest: &PackageManifest,
    name: &str,
    main_body: &str,
    enable_pre_post_scripts: bool,
) -> Vec<(String, String)> {
    let scripts = manifest.value().get("scripts").and_then(Value::as_object);
    let mut stages = vec![(name.to_string(), main_body.to_string())];
    if !enable_pre_post_scripts {
        return stages;
    }
    let pre = format!("pre{name}");
    if let Some(script) = scripts
        .and_then(|scripts| scripts.get(&pre))
        .and_then(Value::as_str)
        .filter(|script| !script.is_empty() && !main_body.contains(&pre))
    {
        stages.insert(0, (pre, script.to_string()));
    }
    let post = format!("post{name}");
    if let Some(script) = scripts
        .and_then(|scripts| scripts.get(&post))
        .and_then(Value::as_str)
        .filter(|script| !script.is_empty() && !main_body.contains(&post))
    {
        stages.push((post, script.to_string()));
    }
    stages
}

/// Run one lifecycle stage. Returns `Ok(None)` when pnpm's per-stage
/// no-op guards apply (empty body, or `npx only-allow pnpm` with no
/// args), so the caller can record "didn't actually run" without
/// inventing a synthetic exit. A non-success [`ScriptExit`] is
/// returned to the caller — single-project `RunArgs::run` exits with
/// the code; recursive `run_recursive` records `Failure` and decides
/// whether to bail.
pub(super) fn run_stage(
    ctx: &RunContext<'_>,
    stage: &str,
    script: &str,
    args: &[String],
) -> miette::Result<Option<ScriptExit>> {
    // The `npx only-allow pnpm` guard script is a no-op, so a lifecycle
    // stage whose final command is exactly that string is skipped. Args
    // are appended *before* this check, so a stage invoked with args
    // (which lengthen the command past the literal) is never skipped;
    // pre/post stages always pass `args = &[]`.
    if args.is_empty() && script == "npx only-allow pnpm" {
        return Ok(None);
    }
    // An empty script body is a no-op: any stage whose (post-arg) command
    // is falsy is skipped, and pre/post are gated on the body being
    // truthy, so an empty `pre<name>`/`post<name>` never runs.
    if script.is_empty() {
        return Ok(None);
    }

    let status = run_script(&RunScript {
        manifest: ctx.manifest.value(),
        stage,
        script,
        args,
        pkg_root: ctx.dir,
        init_cwd: ctx.init_cwd,
        extra_bin_paths: &ctx.config.extra_bin_paths,
        script_shell: ctx.config.script_shell.as_deref().map(Path::new),
        shell_emulator: ctx.config.shell_emulator,
        scripts_prepend_node_path: exec_scripts_prepend_node_path(
            ctx.config.scripts_prepend_node_path,
        ),
        node_execpath: None,
        npm_execpath: None,
        user_agent: Some(&ctx.config.user_agent),
        extra_env: ctx.extra_env,
        silent: ctx.silent,
        output: ctx.output,
        process_tracker: ctx.process_tracker,
    })
    .map_err(miette::Report::new)?;

    if !status.success() {
        // The `test` stage gets a fixed message; a numeric exit code is
        // reported verbatim; a signal-terminated child (no code) is
        // "Command failed." with no number.
        if stage == "test" {
            eprintln!("[ELIFECYCLE] Test failed. See above for more details.");
        } else if let Some(code) = status.code() {
            eprintln!("[ELIFECYCLE] Command failed with exit code {code}.");
        } else {
            eprintln!("[ELIFECYCLE] Command failed.");
        }
    }
    Ok(Some(status))
}

pub(crate) fn exec_scripts_prepend_node_path(
    value: pnpm_config::ScriptsPrependNodePath,
) -> ScriptsPrependNodePath {
    match value {
        pnpm_config::ScriptsPrependNodePath::Always => ScriptsPrependNodePath::Always,
        pnpm_config::ScriptsPrependNodePath::Never => ScriptsPrependNodePath::Never,
        pnpm_config::ScriptsPrependNodePath::WarnOnly => ScriptsPrependNodePath::WarnOnly,
    }
}

/// The `run` positional, resolved once into whichever of pnpm's two
/// readings it is: a script name, or a `/regexp/` matching several.
///
/// Built once per command rather than per project. The recursive runner
/// applies the same selector across every selected project, so compiling
/// the pattern there would repeat the work — and would report a rejected
/// pattern once per project instead of once.
#[derive(Debug)]
pub(super) struct ScriptSelector<'a> {
    name: &'a str,
    /// `None` when the positional is a plain script name — including a
    /// regexp literal whose pattern the engine rejects, which pnpm also
    /// falls back to reading as a name.
    pattern: Option<Regex>,
}

impl<'a> ScriptSelector<'a> {
    pub(super) fn new(name: &'a str) -> Result<ScriptSelector<'a>, RunError> {
        Ok(ScriptSelector { name, pattern: try_build_regex_from_command(name)? })
    }

    /// The script names this selector picks out of `manifest`: an exact
    /// match wins, otherwise every script the pattern matches.
    pub(super) fn select(&self, manifest: &Value) -> Vec<String> {
        let scripts = manifest.get("scripts").and_then(Value::as_object);
        let has_script = scripts
            .and_then(|scripts| scripts.get(self.name))
            .and_then(Value::as_str)
            .is_some_and(|script| !script.is_empty());

        if has_script {
            return vec![self.name.to_string()];
        }
        let (Some(pattern), Some(scripts)) = (self.pattern.as_ref(), scripts) else {
            return Vec::new();
        };
        scripts
            .iter()
            .filter(|(script, body)| {
                body.as_str().is_some_and(|body| !body.is_empty()) && pattern.is_match(script)
            })
            .map(|(script, _)| script.clone())
            .collect()
    }

    /// [`Self::select`] plus single-project `run`'s `start` fallback:
    /// `pnpm start` resolves to `node server.js` even when the manifest
    /// declares no `start` script. The recursive runner has no such
    /// fallback.
    fn select_with_start(&self, manifest: &Value) -> Vec<String> {
        let specified = self.select(manifest);
        if !specified.is_empty() {
            return specified;
        }
        if self.name == "start" {
            return vec![self.name.to_string()];
        }
        Vec::new()
    }
}

/// Compile a `/pattern/` script selector, as pnpm's
/// `tryBuildRegExpFromCommand` does. `Ok(None)` means `command` is not a
/// regexp literal and addresses a script by name; a pattern the engine
/// rejects also reads as a plain name, so a mistyped selector surfaces as
/// the usual "missing script" error rather than a parser diagnostic.
fn try_build_regex_from_command(command: &str) -> Result<Option<Regex>, RunError> {
    let Some((pattern, flags)) = split_regex_literal(command) else {
        return Ok(None);
    };
    // Flags say nothing useful about which scripts to select, so pnpm
    // rejects them rather than silently honouring a subset.
    if !flags.is_empty() {
        return Err(RunError::UnsupportedScriptCommandFormat);
    }
    Ok(Regex::new(pattern).ok())
}

/// Split `/pattern/flags` into its two parts. `None` when `command` is
/// not shaped like a regexp literal: pnpm requires a non-empty pattern
/// whose only `/` characters are backslash-escaped, and flags drawn from
/// JavaScript's flag set. The closing delimiter is therefore the last
/// `/` in the string.
fn split_regex_literal(command: &str) -> Option<(&str, &str)> {
    let body = command.strip_prefix('/')?;
    let close = body.rfind('/')?;
    let (pattern, flags) = body.split_at(close);
    let flags = &flags[1..];
    if pattern.is_empty() || !flags.chars().all(|flag| "dgimuvys".contains(flag)) {
        return None;
    }
    let mut chars = pattern.chars();
    while let Some(char) = chars.next() {
        match char {
            '\\' => {
                chars.next();
            }
            '/' => return None,
            _ => {}
        }
    }
    Some((pattern, flags))
}

/// Drop hidden scripts (names starting with `.`) or reject an explicit
/// request for one.
fn throw_or_filter_hidden_scripts(
    specified: Vec<String>,
    name: &str,
) -> Result<Vec<String>, RunError> {
    if specified.is_empty() || !specified.iter().any(|script| script.starts_with('.')) {
        return Ok(specified);
    }
    if name.starts_with('.') {
        return Err(RunError::HiddenScript { script: name.to_string() });
    }
    let visible: Vec<String> =
        specified.iter().filter(|script| !script.starts_with('.')).cloned().collect();
    if !visible.is_empty() {
        return Ok(visible);
    }
    let hidden_names =
        specified.iter().filter(|s| s.starts_with('.')).map(String::as_str).collect::<Vec<_>>();
    Err(RunError::AllHidden { scripts: hidden_names.join(", ") })
}

/// Render the script listing printed when `pnpm run` is called without a
/// script name.
fn render_project_commands(manifest: &Value, root_manifest: Option<&Value>) -> String {
    let scripts = manifest.get("scripts").and_then(Value::as_object);
    let mut lifecycle = Vec::new();
    let mut other = Vec::new();

    if let Some(scripts) = scripts {
        for (name, script) in scripts {
            if name.starts_with('.') {
                continue;
            }
            let Some(script) = script.as_str() else { continue };
            if ALL_LIFECYCLE_SCRIPTS.contains(&name.as_str()) {
                lifecycle.push((name.as_str(), script));
            } else {
                other.push((name.as_str(), script));
            }
        }
    }

    if lifecycle.is_empty() && other.is_empty() {
        return "There are no scripts specified.".to_string();
    }

    let mut output = String::new();
    if !lifecycle.is_empty() {
        write!(output, "Lifecycle scripts:\n{}", render_commands(&lifecycle)).unwrap();
    }
    if !other.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        write!(output, "Commands available via \"pnpm run\":\n{}", render_commands(&other))
            .unwrap();
    }
    let root_scripts = root_manifest
        .and_then(|manifest| manifest.get("scripts"))
        .and_then(Value::as_object)
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|(name, script)| Some((name.as_str(), script.as_str()?)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !root_scripts.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        write!(
            output,
            "Commands of the root workspace project (to run them, use \"pnpm -w run\"):\n{}",
            render_commands(&root_scripts),
        )
        .unwrap();
    }
    output
}

fn render_commands(commands: &[(&str, &str)]) -> String {
    commands
        .iter()
        .map(|(name, script)| format!("  {name}\n    {script}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lifecycle script names grouped separately in the run listing.
const ALL_LIFECYCLE_SCRIPTS: &[&str] = &[
    "prepublish",
    "prepare",
    "prepublishOnly",
    "prepack",
    "postpack",
    "publish",
    "postpublish",
    "preinstall",
    "install",
    "postinstall",
    "preuninstall",
    "uninstall",
    "postuninstall",
    "preversion",
    "version",
    "postversion",
    "pretest",
    "test",
    "posttest",
    "prestop",
    "stop",
    "poststop",
    "prestart",
    "start",
    "poststart",
    "prerestart",
    "restart",
    "postrestart",
    "preshrinkwrap",
    "shrinkwrap",
    "postshrinkwrap",
];

#[cfg(test)]
mod tests;
