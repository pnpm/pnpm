use super::{
    super::{
        Config, DependencyGroup, HashMap, HashSet, InstallError, NodeLinker, PackageManifest, Path,
        PathBuf, Reporter, RunPostinstallHooks, link_project_bins, run_project_lifecycle_stages,
    },
    project_lifecycle_extra_env,
};

use pnpm_deps_restorer::build_modules::exec_scripts_prepend_node_path;
use pnpm_executor::LifecycleScriptError;
use pnpm_injected_deps_syncer::{SyncInjectedDeps, sync_injected_deps};
use std::sync::Mutex;

/// Run `stages` for the project at `project_dir`, with the workspace root
/// as `INIT_CWD` and the project's own bin dir and `NODE_PATH` in scope.
/// Returns `true` when any script was present and executed.
pub(super) fn run_project_stages(
    config: &Config,
    workspace_root: &Path,
    project_dir: &Path,
    mut extra_env: HashMap<String, String>,
    stages: impl FnOnce(&RunPostinstallHooks<'_>) -> Result<bool, LifecycleScriptError>,
) -> Result<bool, LifecycleScriptError> {
    let root_modules_dir = project_dir.join(config.modules_dir_name());
    let bin_dir = root_modules_dir.join(".bin");
    config.prepend_project_node_path::<pnpm_config::Host>(
        &mut extra_env,
        project_dir,
        config.modules_dir_name(),
    );
    let dep_path = project_dir.to_string_lossy();
    stages(&RunPostinstallHooks {
        environment: pnpm_executor::ScriptEnvironment {
            init_cwd: workspace_root,
            node_execpath: None,
            npm_execpath: None,
            node_gyp_path: None,
            user_agent: Some(&config.user_agent),
            extra_env: &extra_env,
        },
        execution: pnpm_executor::ScriptExecutionOptions {
            extra_bin_paths: &config.extra_bin_paths,
            node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
            prepend_node_path: exec_scripts_prepend_node_path(config),
            shell: config.script_shell.as_deref().map(Path::new),
            shell_emulator: config.shell_emulator,
            wd_bin_dir: Some(&bin_dir),
        },
        dep_path: &dep_path,
        pkg_root: project_dir,
        root_modules_dir: &root_modules_dir,

        unsafe_perm: config.unsafe_perm,

        optional: false,
    })
}

/// What every project's lifecycle run shares: the bin links come first, so
/// a script sees its direct dependencies' bins, then the hooks run with
/// the workspace root as `INIT_CWD`.
pub(super) struct ProjectScriptRunner<'a> {
    config: &'a Config,
    workspace_root: &'a Path,
    normalized_workspace_root: PathBuf,
    /// The root project's `preinstall` ran before the install began, here
    /// (see [`super::run_root_hook`]) or in the CLI that delegated the install,
    /// so its run here starts at `install`.
    root_preinstall_ran: bool,
    extra_env: HashMap<String, String>,
    link_options: pnpm_cmd_shim::LinkBinsOptions,
    /// Every sync relinks every project's `.bin`, and a symlinked bin is
    /// replaced by a remove and a create, so two syncs must not overlap.
    injected_deps_sync: Mutex<()>,
}

impl ProjectScriptRunner<'_> {
    pub(super) fn run<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        manifest: &PackageManifest,
        stages: &[&str],
    ) -> Result<(), InstallError> {
        let root_modules_dir = project_dir.join(self.config.modules_dir_name());
        link_project_bins(&root_modules_dir, &direct_dep_names(manifest), &self.link_options)
            .map_err(InstallError::ProjectBinLink)?;
        let stages = if self.root_preinstall_ran
            && stages.first() == Some(&"preinstall")
            && pnpm_fs::lexical_normalize(project_dir) == self.normalized_workspace_root
        {
            &stages[1..]
        } else {
            stages
        };
        if !self.run_without_bin_linking::<Reporter>(project_dir, stages)? {
            return Ok(());
        }
        // Injected copies are hardlinks of the files that existed when they
        // were materialized, so the output of a build script reaches them
        // only when they are re-synced.
        let _sync_guard =
            self.injected_deps_sync.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        sync_injected_deps(&SyncInjectedDeps {
            pkg_name: manifest
                .value()
                .get("name")
                .and_then(serde_json::Value::as_str),
            pkg_root_dir: project_dir,
            workspace_dir: Some(self.workspace_root),
            modules_dir_name: self.config.modules_dir_name(),
            workspace_modules_dir: &self.config.modules_dir,
            extend_node_path: self.config.extend_node_path,
            manifest_before_scripts: Some(manifest.value()),
            ignored_directories: self.config.managed_directories(),
        })
        .map_err(InstallError::SyncInjectedDeps)
    }

    pub(super) fn run_without_bin_linking<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        stages: &[&str],
    ) -> Result<bool, InstallError> {
        run_project_stages(
            self.config,
            self.workspace_root,
            project_dir,
            self.extra_env.clone(),
            |opts| run_project_lifecycle_stages::<Reporter>(opts, stages),
        )
        .map_err(InstallError::ProjectLifecycleScript)
    }
}

/// Every direct dependency name once, in declaration order across the groups.
fn direct_dep_names(manifest: &PackageManifest) -> Vec<String> {
    let mut direct_dep_names = Vec::new();
    let mut seen = HashSet::new();
    for (name, _) in manifest.dependencies([
        DependencyGroup::Prod,
        DependencyGroup::Dev,
        DependencyGroup::Optional,
    ]) {
        if seen.insert(name) {
            direct_dep_names.push(name.to_string());
        }
    }
    direct_dep_names
}

impl<'a> ProjectScriptRunner<'a> {
    pub(super) fn new(
        config: &'a Config,
        node_linker: NodeLinker,
        workspace_root: &'a Path,
        root_preinstall_ran: bool,
    ) -> Self {
        ProjectScriptRunner {
            config,
            workspace_root,
            normalized_workspace_root: pnpm_fs::lexical_normalize(workspace_root),
            root_preinstall_ran,
            extra_env: project_lifecycle_extra_env(config, node_linker, workspace_root),
            link_options: crate::shim_link_options(config, node_linker),
            injected_deps_sync: Mutex::new(()),
        }
    }
}
