use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_executor::{RunScript, ScriptsPrependNodePath, run_script};
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    fmt::Write as _,
    path::{Path, PathBuf},
};

mod recursive;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A pre-defined package script. When omitted, the available scripts
    /// are listed.
    pub command: Option<String>,

    /// Arguments passed to the script after the script name.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Avoid exiting with a non-zero exit code when the script is undefined.
    #[clap(long)]
    pub if_present: bool,

    /// Run the script starting from the given package, skipping every
    /// package that sorts before it. Only meaningful together with the
    /// global `-r` / `--recursive` flag. Mirrors pnpm's `--resume-from`.
    #[clap(long = "resume-from")]
    pub resume_from: Option<String>,

    /// Save the execution result of every package to
    /// `pnpm-exec-summary.json`. Only meaningful together with the
    /// global `-r` / `--recursive` flag. Mirrors pnpm's
    /// `--report-summary`.
    #[clap(long = "report-summary")]
    pub report_summary: bool,

    /// Keep running the remaining packages after a script fails instead
    /// of aborting on the first failure. Only meaningful together with
    /// the global `-r` / `--recursive` flag. Mirrors pnpm's `--no-bail`
    /// (recursive runs bail by default).
    #[clap(long = "no-bail")]
    pub no_bail: bool,
}

/// Errors from `pacquet run`.
///
/// Mirrors the error codes pnpm raises in its run command
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/run.ts>)
/// and `throwOrFilterHiddenScripts`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/hiddenScripts.ts>).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RunError {
    #[diagnostic(transparent)]
    Manifest(#[error(source)] PackageManifestError),

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
}

impl RunArgs {
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
    pub fn run(self, dir: &Path, config: &Config, silent: bool) -> miette::Result<()> {
        let RunArgs { command, args, if_present, .. } = self;
        let manifest =
            PackageManifest::from_path(dir.join("package.json")).map_err(RunError::Manifest)?;

        let Some(script_name) = command else {
            println!("{}", render_project_commands(manifest.value()));
            return Ok(());
        };

        let mut specified = specified_scripts(manifest.value(), &script_name);

        // Hidden scripts (names starting with `.`) can only be invoked
        // from within another script, detected by an inherited
        // `npm_lifecycle_event`. Mirrors run.ts:231-233.
        if env::var_os("npm_lifecycle_event").is_none() {
            specified = throw_or_filter_hidden_scripts(specified, &script_name)?;
        }

        if specified.is_empty() {
            if if_present {
                return Ok(());
            }
            return Err(RunError::NoScript {
                script: script_name.clone(),
                hint: format!("Command \"{script_name}\" not found."),
            }
            .into());
        }

        let mut extra_env = HashMap::new();
        if let Some(node_options) = &config.node_options {
            extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
        }

        let init_cwd: PathBuf = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
        let ctx = RunContext {
            manifest: &manifest,
            dir,
            init_cwd: &init_cwd,
            config,
            extra_env: &extra_env,
            silent,
        };
        for name in &specified {
            // Resolve the main body (with `start` → `node server.js`
            // fallback) and apply the args-aware `npx only-allow pnpm`
            // no-op skip. After both pass, [`run_stages`] is
            // guaranteed to actually run the main stage, so its return
            // is a plain `ExitStatus`.
            let Some(main) = resolve_main_script(&ctx, name)? else { continue };
            if args.is_empty() && main == "npx only-allow pnpm" {
                continue;
            }
            let status = run_stages(&ctx, name, &main, &args)?;
            if !status.success() {
                // Mirror pnpm: a failing script sets the process exit
                // code. `run_stage` already emitted the `[ELIFECYCLE]`
                // line.
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        Ok(())
    }

    /// Execute the subcommand for every project in the workspace, in
    /// topological order. The recursive counterpart of [`Self::run`],
    /// selected when the global `-r` / `--recursive` flag is set.
    pub fn run_recursive(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        recursive::run_recursive(self, config, dir)
    }
}

/// Shared inputs for running a script, threaded through
/// [`run_stages`] and [`run_stage`] so neither grows an unwieldy
/// argument list. The submodule `recursive` builds a per-project
/// [`RunContext`] and reuses `run_stages`, so the type and its
/// fields are visible up to the parent module.
pub(super) struct RunContext<'a> {
    pub(super) manifest: &'a PackageManifest,
    pub(super) dir: &'a Path,
    pub(super) init_cwd: &'a Path,
    pub(super) config: &'a Config,
    pub(super) extra_env: &'a HashMap<String, String>,
    pub(super) silent: bool,
}

/// Resolve `name` to a runnable main script body, or `Ok(None)` when
/// there's nothing to run (the manifest has no truthy `scripts[name]`
/// and `name` isn't `start`). Mirrors the `start`-fallback path in
/// `runLifecycleHook.ts:75-83`: an absent (or empty) `start` falls back
/// to `node server.js` provided `server.js` exists in the process cwd;
/// otherwise [`RunError::NoScriptOrServer`].
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
            if !ctx.init_cwd.join("server.js").exists() {
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
/// plain [`std::process::ExitStatus`] instead of `Option<ExitStatus>`
/// and the callers don't need to defensively handle a "nothing ran"
/// case.
///
/// On the first non-success stage (pre / main / post) the function
/// short-circuits and returns that stage's status; the caller decides
/// what to do with the failure (single-project: `process::exit`;
/// recursive: record `Failure` and bail or continue). Matches pnpm's
/// `runScript` (run.ts:399-415), where a throw from `runLifecycleHook`
/// skips the remaining stages.
///
/// Deliberate deviation: for `run start` with no `start` script but a
/// `prestart`/`poststart` and `enablePrePostScripts`, pnpm dereferences
/// the undefined `scripts.start` in its `!scripts[name].includes(...)`
/// guard and throws a `TypeError`; pacquet runs the hooks around the
/// `node server.js` fallback instead. Replicating the upstream crash
/// would be wrong, so the `pre`/`post` substring guard runs against
/// the resolved `main_body` here.
pub(super) fn run_stages(
    ctx: &RunContext<'_>,
    name: &str,
    main_body: &str,
    args: &[String],
) -> miette::Result<std::process::ExitStatus> {
    let get_script = |key: &str| -> Option<String> {
        ctx.manifest
            .value()
            .get("scripts")
            .and_then(|scripts| scripts.as_object())
            .and_then(|scripts| scripts.get(key))
            .and_then(|script| script.as_str())
            .map(str::to_string)
    };

    if ctx.config.enable_pre_post_scripts {
        let pre = format!("pre{name}");
        if let Some(script) = get_script(&pre)
            && !main_body.contains(&pre)
            && let Some(status) = run_stage(ctx, &pre, &script, &[])?
            && !status.success()
        {
            return Ok(status);
        }
    }

    // The caller's contract rules out both no-op paths in `run_stage`
    // for the main stage (empty body, args-less `npx only-allow pnpm`),
    // so `run_stage` here is guaranteed to surface a real `ExitStatus`.
    // The `expect` documents the invariant.
    let main_status = run_stage(ctx, name, main_body, args)?.expect(
        "caller validated main_body is neither empty nor the args-less `npx only-allow pnpm` no-op",
    );

    if !main_status.success() {
        return Ok(main_status);
    }

    if ctx.config.enable_pre_post_scripts {
        let post = format!("post{name}");
        if let Some(script) = get_script(&post)
            && !main_body.contains(&post)
            && let Some(status) = run_stage(ctx, &post, &script, &[])?
            && !status.success()
        {
            return Ok(status);
        }
    }

    Ok(main_status)
}

/// Run one lifecycle stage. Returns `Ok(None)` when pnpm's per-stage
/// no-op guards apply (empty body, or `npx only-allow pnpm` with no
/// args), so the caller can record "didn't actually run" without
/// inventing a synthetic `ExitStatus`. A non-success `ExitStatus` is
/// returned to the caller — single-project `RunArgs::run` exits with
/// the code; recursive `run_recursive` records `Failure` and decides
/// whether to bail.
pub(super) fn run_stage(
    ctx: &RunContext<'_>,
    stage: &str,
    script: &str,
    args: &[String],
) -> miette::Result<Option<std::process::ExitStatus>> {
    // The `npx only-allow pnpm` guard script is a no-op under pnpm, so a
    // lifecycle stage whose final command is exactly that string is
    // skipped. pnpm appends args *before* this check
    // (runLifecycleHook.ts:91-100), so a stage invoked with args (which
    // lengthen the command past the literal) is never skipped; pre/post
    // stages always pass `args = &[]`.
    if args.is_empty() && script == "npx only-allow pnpm" {
        return Ok(None);
    }
    // An empty script body is a no-op under pnpm, which skips any stage
    // whose (post-arg) command is falsy (runLifecycleHook.ts:100). pnpm
    // also gates pre/post on the body being truthy (run.ts:403,411), so
    // an empty `pre<name>`/`post<name>` never runs.
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
        scripts_prepend_node_path: exec_scripts_prepend_node_path(
            ctx.config.scripts_prepend_node_path,
        ),
        node_execpath: None,
        npm_execpath: None,
        user_agent: Some("pnpm"),
        extra_env: ctx.extra_env,
        silent: ctx.silent,
    })
    .map_err(miette::Report::new)?;

    if !status.success() {
        // Mirror pnpm's reportLifecycleError (reportError.ts:371-378):
        // the `test` stage gets a fixed message; a numeric exit code is
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

fn exec_scripts_prepend_node_path(
    value: pacquet_config::ScriptsPrependNodePath,
) -> ScriptsPrependNodePath {
    match value {
        pacquet_config::ScriptsPrependNodePath::Always => ScriptsPrependNodePath::Always,
        pacquet_config::ScriptsPrependNodePath::Never => ScriptsPrependNodePath::Never,
        pacquet_config::ScriptsPrependNodePath::WarnOnly => ScriptsPrependNodePath::WarnOnly,
    }
}

/// Resolve which script names to run for `name`. Ports the exact-match
/// arm of pnpm's `getSpecifiedScripts`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/runRecursive.ts#L222-L237>)
/// plus the `start` fallback from run.ts:437-439. The `/regexp/` selector
/// is not ported because pacquet has no regex dependency.
fn specified_scripts(manifest: &Value, name: &str) -> Vec<String> {
    let has_script = manifest
        .get("scripts")
        .and_then(Value::as_object)
        .and_then(|scripts| scripts.get(name))
        .and_then(Value::as_str)
        .is_some_and(|script| !script.is_empty());

    if has_script {
        return vec![name.to_string()];
    }
    if name == "start" {
        return vec![name.to_string()];
    }
    Vec::new()
}

/// Drop hidden scripts (names starting with `.`) or reject an explicit
/// request for one. Ports `throwOrFilterHiddenScripts`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/hiddenScripts.ts>).
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
/// script name. Ports `printProjectCommands`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/run.ts#L348-L387>).
/// The workspace-root section is omitted because pacquet's run has no
/// workspace context yet.
fn render_project_commands(manifest: &Value) -> String {
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
    output
}

fn render_commands(commands: &[(&str, &str)]) -> String {
    commands
        .iter()
        .map(|(name, script)| format!("  {name}\n    {script}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lifecycle script names pnpm groups separately in the run listing.
/// Mirrors `ALL_LIFECYCLE_SCRIPTS`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/run.ts#L314-L346>).
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
