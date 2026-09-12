use super::{
    super::selected_project_indices, dependency_specifier, empty_project,
    saved_dependency_specifier, test_add,
};
use crate::add::{
    manifest::{persist_selected_manifests, prepare_selected_manifests},
    specifier::workspace_save_specifier,
};
use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_network::ThrottledClient;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::SilentReporter;
use std::collections::HashSet;
use tempfile::tempdir;

#[test]
fn explicit_npm_specifier_is_not_rewritten_as_a_workspace_dependency() {
    let mut config = Config::new();
    config.link_workspace_packages = LinkWorkspacePackages::DirectOnly;

    assert_eq!(
        workspace_save_specifier(
            "foo",
            Some("npm:foo@^1.0.0"),
            None,
            &config,
            RangeSpecStyle::Major,
            None,
        ),
        None,
    );
}
#[tokio::test]
async fn selected_add_prepares_and_persists_only_selected_projects() {
    let dir = tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - '*'\n")
        .expect("write workspace manifest");
    let mut projects =
        ["a", "b", "c"].into_iter().map(|name| empty_project(dir.path(), name)).collect::<Vec<_>>();
    let ordered_dirs = [projects[1].root_dir.clone(), projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);
    let config = Box::leak(Box::new(Config::new()));
    let http_client = ThrottledClient::default();

    let packages = ["foo@workspace:*".to_string()];
    let (add, owned) = test_add(config, &http_client, &packages, None);
    prepare_selected_manifests::<SilentReporter>(&mut projects, &indices, add, &owned)
        .await
        .expect("prepare selected manifests");
    persist_selected_manifests::<SilentReporter>(&mut projects, &indices)
        .expect("persist selected manifests");

    assert_eq!(dependency_specifier(&projects[0].manifest, "foo"), Some("workspace:*"));
    assert_eq!(dependency_specifier(&projects[1].manifest, "foo"), Some("workspace:*"));
    assert_eq!(dependency_specifier(&projects[2].manifest, "foo"), None);
    assert_eq!(
        saved_dependency_specifier(&projects[0].manifest, "foo"),
        Some("workspace:*".to_string()),
    );
    assert_eq!(
        saved_dependency_specifier(&projects[1].manifest, "foo"),
        Some("workspace:*".to_string()),
    );
    assert_eq!(saved_dependency_specifier(&projects[2].manifest, "foo"), None);
}
