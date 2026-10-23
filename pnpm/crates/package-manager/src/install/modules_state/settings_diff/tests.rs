use super::{super::super::InstallError, refuse_drifted_modules_dir, same_path};
use pnpm_config::{Config, NodeLinker};
use pnpm_modules_yaml::{LayoutVersion, ModulesLayout, NodeLinker as ModulesNodeLinker};
use std::path::Path;

#[test]
fn same_path_compares_lexically() {
    assert!(same_path(Path::new("/a/store/../store/v11"), Path::new("/a/store/v11")));
    assert!(!same_path(Path::new("/a/store/v11"), Path::new("/a/other/v11")));
}

#[cfg(windows)]
#[test]
fn same_path_ignores_non_ascii_case_on_windows() {
    assert!(same_path(Path::new(r"C:\Störe\v11"), Path::new(r"c:\STÖRE\v11")));
    assert!(!same_path(Path::new(r"C:\Störe\v11"), Path::new(r"C:\Store\v11")));
}

#[test]
fn an_unreadable_modules_manifest_is_refused() {
    let result = refuse_drifted_modules_dir(None, &Config::new(), NodeLinker::Isolated);
    assert!(matches!(result, Err(InstallError::ModulesBreakingChange { .. })), "{result:?}");
}

#[test]
fn a_manifest_without_a_node_linker_is_an_isolated_layout() {
    let config = Config::new();
    let modules = ModulesLayout {
        layout_version: Some(LayoutVersion),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config
            .virtual_store_dir()
            .display()
            .to_string(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        node_linker: None,
        ..ModulesLayout::default()
    };
    let isolated = refuse_drifted_modules_dir(Some(&modules), &config, NodeLinker::Isolated);
    assert!(isolated.is_ok(), "{isolated:?}");
    let hoisted = refuse_drifted_modules_dir(Some(&modules), &config, NodeLinker::Hoisted);
    assert!(matches!(hoisted, Err(InstallError::NodeLinkerDiff)), "{hoisted:?}");
    let recorded_hoisted =
        ModulesLayout { node_linker: Some(ModulesNodeLinker::Hoisted), ..modules };
    let result = refuse_drifted_modules_dir(Some(&recorded_hoisted), &config, NodeLinker::Isolated);
    assert!(matches!(result, Err(InstallError::NodeLinkerDiff)), "{result:?}");
}
