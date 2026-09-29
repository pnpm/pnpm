use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::{Config, NodeLinker};
use std::path::Path;

/// The config-derived options for every bin an install links. Its
/// `extra_node_paths` are [`hoisted_node_paths`] of the install's
/// virtual store dir.
#[must_use]
pub fn shim_link_options(config: &Config, node_linker: NodeLinker) -> LinkBinsOptions {
    let extra_node_paths = hoisted_node_paths(config, node_linker, &config.install_state_dir);
    LinkBinsOptions {
        force: false,
        extra_node_paths,
        prefer_symlinked_executables: config.prefer_symlinked_executables.unwrap_or(false),
        preserve_bin_name: cfg!(unix) && config.preserve_bin_name,
        relocatable_root: config.modules_dir_anchor().map(Path::to_path_buf),
        project_modules_dir_name: (config.extend_node_path
            && config.modules_dir_name() != "node_modules")
            .then(|| config.modules_dir_name().to_owned()),
        installed_modules_dir: (config.modules_dir_name() != "node_modules").then(|| {
            config.modules_dir.clone()
        }),
    }
}

/// pnpm's `getExtraNodePaths`: under the isolated linker with a hoist
/// pattern, shims carry the hidden hoisted modules dir
/// (`<virtual_store_dir>/node_modules`) on `NODE_PATH`, unless
/// `extendNodePath: false`. Everything else (the hoisted linker, a
/// disabled hoist pass) gets no `NODE_PATH` at all.
#[must_use]
pub fn hoisted_node_paths(
    config: &Config,
    node_linker: NodeLinker,
    virtual_store_dir: &Path,
) -> Vec<String> {
    let has_hoist_pattern = config.hoist_pattern
        .as_ref()
        .is_some_and(|patterns| !patterns.is_empty());
    if config.extend_node_path && matches!(node_linker, NodeLinker::Isolated) && has_hoist_pattern {
        vec![
            virtual_store_dir
                .join("node_modules")
                .to_string_lossy()
                .into_owned(),
        ]
    } else {
        Vec::new()
    }
}
