use super::VersionChange;
use miette::Context;
use pnpm_config::Config;
use pnpm_executor::{RunPostinstallHooks, run_lifecycle_hook};
use pnpm_package_manifest::PackageManifest;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Run one `preversion` / `version` / `postversion` script of the bumped
/// package, when the manifest declares it, scripts are not ignored and this
/// is not a dry run. The manifest is re-read so the `version` and
/// `postversion` hooks see the bumped version.
pub(super) fn run_version_lifecycle_hook<Reporter: pnpm_reporter::Reporter>(
    stage: &str,
    change: &VersionChange,
    config: &Config,
    init_cwd: &Path,
    dry_run: bool,
) -> miette::Result<()> {
    if config.ignore_scripts || dry_run {
        return Ok(());
    }
    let manifest = PackageManifest::from_path(change.manifest_path.clone())
        .wrap_err_with(|| format!("reading {}", change.manifest_path.display()))?;
    let Some(script) = manifest
        .value()
        .get("scripts")
        .and_then(|scripts| scripts.get(stage))
        .and_then(Value::as_str)
        .filter(|script| !script.is_empty())
        .map(ToString::to_string)
    else {
        return Ok(());
    };

    let root_modules_dir = change.path.join(&config.modules_dir);
    let (bin_dir, extra_env) = project_scripts_bin_dir_and_env(change, config);
    let script_shell = config.script_shell.as_ref().map(PathBuf::from);
    let run_opts = RunPostinstallHooks {
        environment: crate::cli_args::run::script_environment(config, init_cwd, &extra_env),
        execution: pnpm_executor::ScriptExecutionOptions {
            extra_bin_paths: &config.extra_bin_paths,
            node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
            prepend_node_path: crate::cli_args::run::exec_scripts_prepend_node_path(
                config.scripts_prepend_node_path,
            ),
            shell: script_shell.as_deref(),
            shell_emulator: config.shell_emulator,
            wd_bin_dir: Some(&bin_dir),
        },
        dep_path: &change.name,
        pkg_root: &change.path,
        root_modules_dir: &root_modules_dir,

        unsafe_perm: config.unsafe_perm,

        optional: false,
    };
    let parent_env: HashMap<String, String> = std::env::vars().collect();
    run_lifecycle_hook::<Reporter>(stage, &script, &run_opts, manifest.value(), &parent_env)
        .map_err(miette::Report::new)
}

fn project_scripts_bin_dir_and_env(
    change: &VersionChange,
    config: &Config,
) -> (PathBuf, HashMap<String, String>) {
    let modules_dir_name = config.modules_dir_name_for(&change.path, Some(&change.name));
    let mut extra_env = config.extra_env.clone();
    config.prepend_project_node_path::<pnpm_config::Host>(
        &mut extra_env,
        &change.path,
        &modules_dir_name,
    );
    (change.path.join(modules_dir_name).join(".bin"), extra_env)
}
