use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_executor::{RunScript, ScriptsPrependNodePath, run_script};
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use serde_json::Value;
use std::{collections::HashMap, env, path::Path};

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
        help("Scripts starting with \".\" are hidden and can only be called from other scripts.")
    )]
    HiddenScript { script: String },
}

impl RunArgs {
    /// Execute the subcommand in `dir`. `silent` suppresses the
    /// `$ <script>` echo (set when the reporter is `silent`).
    ///
    /// On a non-zero script exit code this terminates the process with
    /// the same code, matching pnpm where a failing script sets the
    /// process exit code.
    pub fn run(self, dir: &Path, config: &Config, silent: bool) -> miette::Result<()> {
        let RunArgs { command, args, if_present } = self;
        let manifest =
            PackageManifest::from_path(dir.join("package.json")).map_err(RunError::Manifest)?;

        let Some(script_name) = command else {
            println!("{}", render_project_commands(&manifest));
            return Ok(());
        };

        let mut specified = specified_scripts(&manifest, &script_name);

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

        let ctx = RunContext { manifest: &manifest, dir, config, extra_env: &extra_env, silent };
        for name in &specified {
            run_one_script(&ctx, name, &args)?;
        }
        Ok(())
    }
}

/// Shared inputs for running a script, threaded through
/// [`run_one_script`] and [`run_stage`] so neither grows an unwieldy
/// argument list.
struct RunContext<'a> {
    manifest: &'a PackageManifest,
    dir: &'a Path,
    config: &'a Config,
    extra_env: &'a HashMap<String, String>,
    silent: bool,
}

/// Run a single named script together with its `pre`/`post` companions
/// when `enablePrePostScripts` is set. Ports `runScript`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/run.ts#L395-L423>).
fn run_one_script(ctx: &RunContext<'_>, name: &str, args: &[String]) -> miette::Result<()> {
    let get_script = |key: &str| -> Option<String> {
        ctx.manifest
            .value()
            .get("scripts")
            .and_then(|scripts| scripts.as_object())
            .and_then(|scripts| scripts.get(key))
            .and_then(|script| script.as_str())
            .map(str::to_string)
    };

    // `start` falls back to `node server.js`, matching pnpm's
    // getSpecifiedScripts + runLifecycleHook start handling.
    let Some(main) =
        get_script(name).or_else(|| (name == "start").then(|| "node server.js".into()))
    else {
        return Ok(());
    };

    if ctx.config.enable_pre_post_scripts {
        let pre = format!("pre{name}");
        if let Some(script) = get_script(&pre)
            && !main.contains(&pre)
        {
            run_stage(ctx, &pre, &script, &[])?;
        }
    }

    // The `npx only-allow pnpm` guard script is a no-op under pnpm, so
    // it is skipped. Mirrors runLifecycleHook.ts:100.
    if main != "npx only-allow pnpm" {
        run_stage(ctx, name, &main, args)?;
    }

    if ctx.config.enable_pre_post_scripts {
        let post = format!("post{name}");
        if let Some(script) = get_script(&post)
            && !main.contains(&post)
        {
            run_stage(ctx, &post, &script, &[])?;
        }
    }

    Ok(())
}

/// Run one lifecycle stage and propagate its exit code.
fn run_stage(
    ctx: &RunContext<'_>,
    stage: &str,
    script: &str,
    args: &[String],
) -> miette::Result<()> {
    let status = run_script(RunScript {
        manifest: ctx.manifest.value(),
        stage,
        script,
        args,
        pkg_root: ctx.dir,
        init_cwd: ctx.dir,
        extra_bin_paths: &ctx.config.extra_bin_paths,
        script_shell: ctx.config.script_shell.as_deref().map(Path::new),
        scripts_prepend_node_path: exec_scripts_prepend_node_path(
            ctx.config.scripts_prepend_node_path,
        ),
        node_execpath: None,
        npm_execpath: None,
        user_agent: None,
        extra_env: ctx.extra_env,
        silent: ctx.silent,
    })
    .map_err(miette::Report::new)?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
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
fn specified_scripts(manifest: &PackageManifest, name: &str) -> Vec<String> {
    let has_script = manifest
        .value()
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
        specified.into_iter().filter(|script| !script.starts_with('.')).collect();
    if visible.is_empty() {
        return Err(RunError::HiddenScript { script: name.to_string() });
    }
    Ok(visible)
}

/// Render the script listing printed when `pnpm run` is called without a
/// script name. Ports `printProjectCommands`
/// (<https://github.com/pnpm/pnpm/blob/d4a2b0364c/exec/commands/src/run.ts#L348-L387>).
/// The workspace-root section is omitted because pacquet's run has no
/// workspace context yet.
fn render_project_commands(manifest: &PackageManifest) -> String {
    let scripts = manifest.value().get("scripts").and_then(Value::as_object);
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
        output.push_str(&format!("Lifecycle scripts:\n{}", render_commands(&lifecycle)));
    }
    if !other.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!(
            "Commands available via \"pnpm run\":\n{}",
            render_commands(&other)
        ));
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
const ALL_LIFECYCLE_SCRIPTS: [&str; 31] = [
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
