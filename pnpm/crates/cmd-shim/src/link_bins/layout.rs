use super::LinkBinsOptions;
use std::path::Path;

/// What the layout [`bin_layout_fingerprint`] keys on: the parts of
/// [`LinkBinsOptions`] that change what a linked bin looks like on disk.
/// Absolute `extra_node_paths` collapse to one placeholder so a cache-local
/// store or temp directory does not read as a different layout, and the
/// settings that only mean something on Unix contribute nothing there.
#[must_use]
pub fn bin_layout_fingerprint(options: &LinkBinsOptions) -> String {
    let extra_node_paths =
        options.extra_node_paths
            .iter()
            .map(|path| {
                if Path::new(path).is_absolute() { "<absolute>".to_string() } else { path.clone() }
            })
            .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({
        "extraNodePaths": extra_node_paths,
        "preferSymlinkedExecutables": options.prefer_symlinked_executables && cfg!(unix),
        "preserveBinName": options.preserve_bin_name && cfg!(unix),
        "relocatableRoot": options.relocatable_root.is_some(),
        "projectModulesDirName": options.project_modules_dir_name.as_ref().map(|name| name.to_string_lossy()),
        "installedModulesDir": options.installed_modules_dir.is_some(),
    }))
    .expect("serialize bin layout fingerprint")
}
