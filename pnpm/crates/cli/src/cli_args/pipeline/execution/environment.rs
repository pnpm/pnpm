use super::{
    Config, HashMap, Path, RunContext, RunTaskOptions, ScriptOutput, capture, cargo_cache,
    make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
    pnp_path_for_execution,
};

/// The task's environment with Cargo pointed at the cached build directory.
pub(super) fn cargo_build_env(
    base_extra_env: &HashMap<String, String>,
    cargo: &cargo_cache::CargoCache,
) -> HashMap<String, String> {
    let mut extra_env = base_extra_env.clone();
    for name in ["CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"] {
        extra_env.insert(
            name.to_string(),
            cargo.target.to_string_lossy().into_owned(),
        );
    }
    extra_env
}

pub(in super::super) fn task_environment(
    config: &Config,
    root: &Path,
    base_extra_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut extra_env = base_extra_env.clone();
    if let Some(pnp_path) = pnp_path_for_execution(config, root) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_require_option(&pnp_path, node_options),
        );
    }
    if let Some(package_map_path) = package_map_path_for_execution(config, root) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_package_map_option(&package_map_path, node_options),
        );
    }

    extra_env
}

pub(super) fn pipeline_script_context<'a>(
    options: &'a RunTaskOptions<'_, '_>,
    extra_env: &'a HashMap<String, String>,
    root_str: &'a str,
    capture_output: bool,
) -> RunContext<'a> {
    let root = options.node.project.as_path();
    RunContext {
        manifest: &options.graph[root].package.project.manifest,
        dir: root,
        init_cwd: options.environment.init_cwd,
        config: options.config,
        extra_env,
        silent: options.reporting.silent,
        output: ScriptOutput::Streamed {
            dep_path: root_str,
            emit: if capture_output {
                capture::capturing_emit
            } else {
                options.reporting.emit
            },
        },
        // The pipeline never bails, so there is no cancellation to
        // propagate into running children.
        process_tracker: None,
    }
}
