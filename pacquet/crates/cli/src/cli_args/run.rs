use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pacquet_config::{Config, ScriptsPrependNodePath as ConfigScriptsPrependNodePath};
use pacquet_executor::{RunScript, ScriptsPrependNodePath, run_script};
use pacquet_package_manifest::PackageManifest;
use serde_json::Value;
use std::{collections::HashMap, path::Path};

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A pre-defined package script. When omitted, the available scripts
    /// are printed instead of running anything.
    pub command: Option<String>,

    /// Any additional arguments passed after the script name.
    pub args: Vec<String>,

    /// Avoid exiting with a non-zero exit code when the script is
    /// undefined. This lets you run potentially undefined scripts
    /// without breaking the execution chain.
    #[clap(long)]
    pub if_present: bool,
}

/// Error type of [`RunArgs::run`]. Mirrors the `PnpmError` codes pnpm's
/// `run` command raises.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RunError {
    #[display("Missing script: {script_name}")]
    #[diagnostic(code(ERR_PNPM_NO_SCRIPT), help("Command \"{script_name}\" not found."))]
    NoScript { script_name: String },

    #[display("Script \"{script_name}\" is hidden and cannot be run directly")]
    #[diagnostic(
        code(ERR_PNPM_HIDDEN_SCRIPT),
        help("Scripts starting with \".\" are hidden and can only be called from other scripts.")
    )]
    HiddenScript { script_name: String },

    #[display("All matched scripts are hidden and cannot be run directly: {scripts}")]
    #[diagnostic(
        code(ERR_PNPM_HIDDEN_SCRIPT),
        help("Scripts starting with \".\" are hidden and can only be called from other scripts.")
    )]
    AllHidden { scripts: String },
}

impl RunArgs {
    /// Execute the subcommand against the project rooted at `dir`.
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        let RunArgs { command, args, if_present } = self;

        let manifest = PackageManifest::from_path(dir.join("package.json"))
            .wrap_err("getting the package.json in current directory")?;

        let Some(script_name) = command else {
            println!("{}", print_project_commands(manifest.value()));
            return Ok(());
        };

        let mut specified = get_specified_scripts(manifest.value(), &script_name);

        // Hidden scripts (names starting with ".") can only be invoked
        // from within another script — i.e. when a lifecycle event is
        // already in progress. Mirrors upstream's
        // `if (!process.env.npm_lifecycle_event)` guard.
        if std::env::var_os("npm_lifecycle_event").is_none() {
            specified = throw_or_filter_hidden_scripts(specified, &script_name)?;
        }

        if specified.is_empty() {
            if if_present {
                return Ok(());
            }
            return Err(RunError::NoScript { script_name }.into());
        }

        let init_cwd = std::env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
        let mut extra_env: HashMap<String, String> = HashMap::new();
        if let Some(node_options) = &config.node_options {
            extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
        }
        let scripts_prepend_node_path =
            map_scripts_prepend_node_path(config.scripts_prepend_node_path);

        for script in &specified {
            run_one(RunOne {
                manifest: &manifest,
                script_name: script,
                args: &args,
                dir,
                init_cwd: &init_cwd,
                extra_env: &extra_env,
                scripts_prepend_node_path,
                config,
            })?;
        }

        Ok(())
    }
}

struct RunOne<'a> {
    manifest: &'a PackageManifest,
    script_name: &'a str,
    args: &'a [String],
    dir: &'a Path,
    init_cwd: &'a Path,
    extra_env: &'a HashMap<String, String>,
    scripts_prepend_node_path: ScriptsPrependNodePath,
    config: &'a Config,
}

/// Run a single named script, plus its `pre`/`post` siblings when
/// `enablePrePostScripts` is set. Ports `runScript` from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/run.ts#L395-L423>.
fn run_one(opts: RunOne<'_>) -> miette::Result<()> {
    let scripts = opts.manifest.value().get("scripts").and_then(Value::as_object);
    let get = |name: &str| -> Option<&str> {
        scripts.and_then(|map| map.get(name)).and_then(Value::as_str)
    };

    let main_script = get(opts.script_name);

    if opts.config.enable_pre_post_scripts {
        let pre = format!("pre{}", opts.script_name);
        // pnpm guards `pre`/`post` on the main script not already
        // referencing them (so a script that calls its own pre-hook
        // isn't run twice).
        if let Some(pre_script) = get(&pre)
            && main_script.is_none_or(|main| !main.contains(&pre))
        {
            exec_stage(&opts, &pre, pre_script, &[])?;
        }
    }

    if let Some(main_script) = main_script {
        exec_stage(&opts, opts.script_name, main_script, opts.args)?;
    } else if opts.script_name == "start" {
        // `pnpm run start` with no `start` script falls back to
        // `node server.js`, matching upstream's lifecycle default at
        // <https://github.com/pnpm/pnpm/blob/80037699fb/exec/lifecycle/src/runLifecycleHook.ts#L77-L83>.
        exec_stage(&opts, "start", "node server.js", opts.args)?;
    }

    if opts.config.enable_pre_post_scripts && main_script.is_some() {
        let post = format!("post{}", opts.script_name);
        if let Some(post_script) = get(&post)
            && main_script.is_none_or(|main| !main.contains(&post))
        {
            exec_stage(&opts, &post, post_script, &[])?;
        }
    }

    Ok(())
}

/// Spawn one script stage in the foreground. On a non-zero exit, print
/// pnpm's `ELIFECYCLE` line and exit the process with the script's code
/// — matching pnpm, which propagates the failing script's exit code.
fn exec_stage(opts: &RunOne<'_>, stage: &str, script: &str, args: &[String]) -> miette::Result<()> {
    let status = run_script(RunScript {
        pkg_root: opts.dir,
        stage,
        script,
        args,
        manifest: opts.manifest.value(),
        init_cwd: opts.init_cwd,
        extra_bin_paths: &opts.config.extra_bin_paths,
        extra_env: opts.extra_env,
        node_execpath: None,
        npm_execpath: None,
        user_agent: Some("pnpm"),
        scripts_prepend_node_path: opts.scripts_prepend_node_path,
        script_shell: opts.config.script_shell.as_deref(),
        print_command: true,
    })
    .map_err(miette::Report::new)?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        eprintln!(" ELIFECYCLE  Command failed with exit code {code}.");
        std::process::exit(code);
    }
    Ok(())
}

/// Map the config-side tri-state onto the executor's enum. Mirrors the
/// match in `install_frozen_lockfile.rs`.
fn map_scripts_prepend_node_path(value: ConfigScriptsPrependNodePath) -> ScriptsPrependNodePath {
    match value {
        ConfigScriptsPrependNodePath::Always => ScriptsPrependNodePath::Always,
        ConfigScriptsPrependNodePath::Never => ScriptsPrependNodePath::Never,
        ConfigScriptsPrependNodePath::WarnOnly => ScriptsPrependNodePath::WarnOnly,
    }
}

/// Resolve which script names the user's selector matches. Ports the
/// exact-match + `start`-fallback arms of upstream's `getSpecifiedScripts`
/// at <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/run.ts#L429-L442>.
///
/// The RegExp-literal selector (`/build:.*/`) is not yet ported — pacquet
/// has no regex engine in its dependency set — so a `/.../`-shaped
/// selector falls through to the "missing script" path.
fn get_specified_scripts(manifest: &Value, script_name: &str) -> Vec<String> {
    let scripts = manifest.get("scripts").and_then(Value::as_object);
    if scripts.and_then(|map| map.get(script_name)).and_then(Value::as_str).is_some() {
        return vec![script_name.to_string()];
    }
    if script_name == "start" {
        return vec![script_name.to_string()];
    }
    Vec::new()
}

/// Filter hidden scripts (names starting with ".") when invoked outside a
/// lifecycle. Throws if a hidden script was requested by exact name, or
/// if every matched script is hidden. Ports
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/hiddenScripts.ts>.
fn throw_or_filter_hidden_scripts(
    specified: Vec<String>,
    script_name: &str,
) -> Result<Vec<String>, RunError> {
    if specified.is_empty() {
        return Ok(specified);
    }
    let has_hidden = specified.iter().any(|s| s.starts_with('.'));
    if !has_hidden {
        return Ok(specified);
    }
    if script_name.starts_with('.') {
        return Err(RunError::HiddenScript { script_name: script_name.to_string() });
    }
    let visible: Vec<String> = specified.iter().filter(|s| !s.starts_with('.')).cloned().collect();
    if !visible.is_empty() {
        return Ok(visible);
    }
    let hidden: Vec<&str> =
        specified.iter().filter(|s| s.starts_with('.')).map(String::as_str).collect();
    Err(RunError::AllHidden { scripts: hidden.join(", ") })
}

/// Lifecycle script names pnpm groups separately when listing scripts.
/// Ports `ALL_LIFECYCLE_SCRIPTS` from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/run.ts#L314-L346>.
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

/// Render the project's scripts, split into lifecycle and other
/// commands. Ports `printProjectCommands` from
/// <https://github.com/pnpm/pnpm/blob/80037699fb/exec/commands/src/run.ts#L348-L387>
/// (single-project: the workspace-root section is omitted).
fn print_project_commands(manifest: &Value) -> String {
    let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
        return "There are no scripts specified.".to_string();
    };

    let mut lifecycle: Vec<(&str, &str)> = Vec::new();
    let mut other: Vec<(&str, &str)> = Vec::new();
    for (name, body) in scripts {
        if name.starts_with('.') {
            continue;
        }
        let Some(body) = body.as_str() else { continue };
        if ALL_LIFECYCLE_SCRIPTS.contains(&name.as_str()) {
            lifecycle.push((name, body));
        } else {
            other.push((name, body));
        }
    }

    if lifecycle.is_empty() && other.is_empty() {
        return "There are no scripts specified.".to_string();
    }

    let mut output = String::new();
    if !lifecycle.is_empty() {
        output.push_str("Lifecycle scripts:\n");
        output.push_str(&render_commands(&lifecycle));
    }
    if !other.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str("Commands available via \"pnpm run\":\n");
        output.push_str(&render_commands(&other));
    }
    output
}

fn render_commands(commands: &[(&str, &str)]) -> String {
    commands
        .iter()
        .map(|(name, body)| format!("  {name}\n    {body}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
