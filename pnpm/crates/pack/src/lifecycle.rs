use super::{
    Arc, HashMap, HookContext, HookLog, LogEvent, LogFn, LogLevel, PackError, PackScripts, Path,
    PnpmfileHooks, Reporter, RunPostinstallHooks, ScriptsPrependNodePath, Value, realpath_missing,
    run_lifecycle_hook,
};

/// Chain every configured pnpmfile's `beforePacking` hook over the
/// published `manifest`, in order. `project_dir` is the packed project's
/// root (used as the log prefix); `publish_dir` is the directory passed
/// to the hook (the project's publish directory, honoring
/// `publishConfig.directory`), matching pnpm's `hook(manifest, dir)`.
pub(super) async fn apply_before_packing<Reporter: self::Reporter>(
    project_dir: &Path,
    publish_dir: &Path,
    mut manifest: Value,
    hooks: &[Arc<dyn PnpmfileHooks>],
) -> Result<Value, PackError> {
    let prefix = project_dir.to_string_lossy();
    for hook in hooks {
        let pnpmfile = hook.source_path().unwrap_or_else(|| Path::new("<pnpmfile>"));
        let ctx =
            HookContext { log: before_packing_logger::<Reporter>(pnpmfile, &prefix), dir: None };
        manifest = hook
            .before_packing(manifest, publish_dir, ctx)
            .await
            .map_err(|err| PackError::BeforePacking {
                pnpmfile: pnpmfile.display().to_string(),
                message: err.to_string(),
            })?;
    }
    Ok(manifest)
}

/// A `context.log(...)` sink forwarding each `beforePacking` log line to
/// the `pnpm:hook` channel, tagged with the pnpmfile it came from.
fn before_packing_logger<Reporter: self::Reporter>(pnpmfile: &Path, prefix: &str) -> LogFn {
    let from = pnpmfile.to_string_lossy().into_owned();
    let prefix = prefix.to_owned();
    Arc::new(move |message| {
        Reporter::emit(&LogEvent::Hook(HookLog {
            level: LogLevel::Debug,
            from: from.clone(),
            hook: "beforePacking".to_string(),
            prefix: prefix.clone(),
            message,
        }));
    })
}

impl PackScripts {
    /// Run the named lifecycle scripts that the manifest actually declares,
    /// in order. Mirrors upstream's `runScriptsIfPresent`; the Rust port is
    /// a plain loop rather than upstream's bound partial application.
    pub(super) fn run_if_present<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        script_names: &[&str],
        manifest: &Value,
    ) -> Result<(), PackError> {
        let scripts = manifest.get("scripts");
        if !script_names
            .iter()
            .any(|name| script_body(scripts, name).is_some())
        {
            return Ok(());
        }

        let dep_path = dir.to_string_lossy().into_owned();
        let root_modules_dir = realpath_missing(&dir.join("node_modules"));
        let run_opts = RunPostinstallHooks {
            environment: pnpm_executor::ScriptEnvironment {
                init_cwd: dir,
                node_execpath: None,
                npm_execpath: None,
                node_gyp_path: None,
                user_agent: Some(&self.user_agent),
                extra_env: &self.extra_env,
            },
            execution: pnpm_executor::ScriptExecutionOptions {
                extra_bin_paths: &self.extra_bin_paths,
                node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
                prepend_node_path: ScriptsPrependNodePath::default(),
                shell: None,
                shell_emulator: false,
            },
            dep_path: &dep_path,
            pkg_root: dir,
            root_modules_dir: &root_modules_dir,

            unsafe_perm: self.unsafe_perm,

            optional: false,
        };
        let parent_env: HashMap<String, String> = std::env::vars().collect();

        for &script_name in script_names {
            let Some(script) = script_body(scripts, script_name) else { continue };
            run_lifecycle_hook::<Reporter>(script_name, script, &run_opts, manifest, &parent_env)
                .map_err(PackError::Lifecycle)?;
        }
        Ok(())
    }
}

/// The body of `scripts.<name>` when it is a non-empty string.
pub(super) fn script_body<'a>(scripts: Option<&'a Value>, name: &str) -> Option<&'a str> {
    scripts?
        .get(name)
        .and_then(Value::as_str)
        .filter(|script| !script.is_empty())
}
