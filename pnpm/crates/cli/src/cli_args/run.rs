use super::exec::ExecArgs;
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_executor::{RunScript, ScriptsPrependNodePath, run_script};
use pacquet_package_manager::{make_node_package_map_option, package_map_path_for_execution};
use pacquet_package_manifest::PackageManifest;
use pacquet_workspace::{ReadProjectManifestOnlyError, read_project_manifest_only};
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

    /// Run the specified scripts one by one.
    #[clap(long, short = 's')]
    pub sequential: bool,
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
}

impl RunArgs {
    /// Execute the subcommand in `dir`.
    pub fn run(self, dir: &Path, config: &Config, silent: bool) -> miette::Result<()> {
        self.run_inner(dir, config, silent, false)
    }

    pub fn run_fallback(self, dir: &Path, config: &Config, silent: bool) -> miette::Result<()> {
        self.run_inner(dir, config, silent, true)
    }

    fn run_inner(
        self,
        dir: &Path,
        config: &Config,
        silent: bool,
        fallback_to_exec: bool,
    ) -> miette::Result<()> {
        // Verify deps before reading manifest so a mistyped command in a project-less directory skips the install check.
        super::verify_deps::verify_deps_before_run(dir, config, silent)?;
        let RunArgs { command, args, if_present, sequential, .. } = self;
        let Some(script_name) = command else {
            let manifest = read_project_manifest_only(dir).map_err(RunError::Manifest)?;
            println!("{}", render_project_commands(manifest.value()));
            return Ok(());
        };
        let manifest = match read_project_manifest_only(dir) {
            Ok(manifest) => manifest,
            Err(ReadProjectManifestOnlyError::NoImporterManifestFound { .. })
                if fallback_to_exec =>
            {
                return exec_fallback(script_name, args, dir, config);
            }
            Err(err) => return Err(RunError::Manifest(err).into()),
        };

        let mut specified = specified_scripts(manifest.value(), &script_name, !sequential);

        // Hidden scripts (names starting with `.`) can only be invoked from within another script, detected by an inherited `npm_lifecycle_event`.
        if env::var_os("npm_lifecycle_event").is_none() {
            specified = throw_or_filter_hidden_scripts(specified, &script_name)?;
        }

        if specified.is_empty() {
            if if_present {
                return Ok(());
            }
            if fallback_to_exec {
                return exec_fallback(script_name, args, dir, config);
            }
            return Err(RunError::NoScript {
                script: script_name.clone(),
                hint: format!(r#"Command "{script_name}" not found."#),
            }
            .into());
        }

        let mut extra_env = config.extra_env.clone();
        if let Some(node_options) = &config.node_options {
            extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
        }
        if let Some(package_map_path) = package_map_path_for_execution(config, dir) {
            let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
            extra_env.insert(
                "NODE_OPTIONS".to_string(),
                make_node_package_map_option(&package_map_path, node_options),
            );
        }

        let init_cwd: PathBuf = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
        let ctx = RunContext {
            manifest: &manifest,
            dir,
            init_cwd: &init_cwd,
            config,
            extra_env: &extra_env,
            silent,
            sequential,
        };
        for name in &specified {
            let Some(main) = resolve_main_script(&ctx, name)? else { continue };
            // Skip the no-op `npx only-allow pnpm` guard when no args.
            if args.is_empty() && main == "npx only-allow pnpm" {
                continue;
            }
            let status = run_stages(&ctx, name, &main, &args)?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        Ok(())
    }

    /// Execute the subcommand across `--filter`-selected workspace projects.
    pub fn run_recursive(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        super::verify_deps::verify_deps_before_run(dir, config, false)?;
        recursive::run_recursive(self, config, dir)
    }
}

fn exec_fallback(
    script_name: String,
    args: Vec<String>,
    dir: &Path,
    config: &Config,
) -> miette::Result<()> {
    ExecArgs {
        command: std::iter::once(script_name).chain(args).collect(),
        shell_mode: false,
        resume_from: None,
        report_summary: false,
        no_bail: false,
        sort: true,
    }
    .run(dir, config)
}

/// Shared inputs for running a script, threaded through [`run_stages`] and [`run_stage`].
pub(super) struct RunContext<'a> {
    pub(super) manifest: &'a PackageManifest,
    pub(super) dir: &'a Path,
    pub(super) init_cwd: &'a Path,
    pub(super) config: &'a Config,
    pub(super) extra_env: &'a HashMap<String, String>,
    pub(super) silent: bool,
    pub(super) sequential: bool,
}

/// Resolve `name` to a runnable main script body, or `Ok(None)` when there's nothing to run.
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

/// Run pre / main / post for `name`. On the first non-success stage, short-circuit and skip the rest.
pub(super) fn run_stages(
    ctx: &RunContext<'_>,
    name: &str,
    main_body: &str,
    args: &[String],
) -> miette::Result<std::process::ExitStatus> {
    let _ = ctx.sequential;
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

    // The caller validates main_body is non-empty and not the args-less
    // npx-only-allow no-op, so `run_stage` here is guaranteed to
    // surface a real `ExitStatus`.
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

/// Run one lifecycle stage. Returns `Ok(None)` for no-op guards
/// (empty body, or `npx only-allow pnpm` with no args).
pub(super) fn run_stage(
    ctx: &RunContext<'_>,
    stage: &str,
    script: &str,
    args: &[String],
) -> miette::Result<Option<std::process::ExitStatus>> {
    if args.is_empty() && script == "npx only-allow pnpm" {
        return Ok(None);
    }
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

/// Resolve script names for `name`: exact match, `/regexp/` selector, or `start` fallback.
fn specified_scripts(manifest: &Value, name: &str, sort: bool) -> Vec<String> {
    if let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) {
        if let Some(entry) = scripts.get(name).and_then(Value::as_str)
            && !entry.is_empty()
        {
            return vec![name.to_string()];
        }

        if let Some(pattern) = parse_regexp_selector(name) {
            match regex::Regex::new(&pattern) {
                Ok(re) => {
                    let mut keys: Vec<String> = scripts
                        .keys()
                        .filter(|script_key| re.is_match(script_key.as_str()))
                        .cloned()
                        .collect();
                    if sort {
                        keys.sort();
                    }
                    return keys;
                }
                Err(_) => return Vec::new(),
            }
        }
    }

    if name == "start" {
        return vec![name.to_string()];
    }
    Vec::new()
}

/// Parse a `/pattern/` selector. Returns `None` when `name` is not in regexp format, has no closing `/`, or carries flags.
fn parse_regexp_selector(name: &str) -> Option<String> {
    let rest = name.strip_prefix('/')?;
    let (pattern, flags) = rest.rsplit_once('/')?;
    if !flags.is_empty() {
        return None;
    }
    Some(pattern.to_string())
}

/// Drop hidden scripts (names starting with `.`) or reject an explicit request for one.
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

/// Render the script listing printed when `pnpm run` is called without a script name.
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

/// Lifecycle script names grouped separately in the run listing.
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
