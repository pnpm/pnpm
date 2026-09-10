use super::{super::selected_project_indices, dependency_specifier, project_with_foo, test_add};
use crate::add::manifest::prepare_selected_manifests;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_reporter::SilentReporter;
use std::collections::HashSet;
use tempfile::tempdir;

#[tokio::test]
async fn selected_add_merges_catalog_updates_in_command_order() {
    let dir = tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - '*'\n")
        .expect("write workspace manifest");
    let mut projects = vec![
        project_with_foo(dir.path(), "a", "1.0.0"),
        project_with_foo(dir.path(), "b", "2.0.0"),
    ];
    let ordered_dirs = [projects[1].root_dir.clone(), projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);
    let config = Box::leak(Box::new(Config::new()));
    let http_client = ThrottledClient::default();

    let packages = ["foo".to_string()];
    let (add, owned) = test_add(config, &http_client, &packages, Some("default"));
    let prepared =
        prepare_selected_manifests::<SilentReporter>(&mut projects, &indices, add, &owned)
            .await
            .expect("prepare selected manifests");

    assert_eq!(dependency_specifier(&projects[0].manifest, "foo"), Some("1.0.0"));
    assert_eq!(dependency_specifier(&projects[1].manifest, "foo"), Some("catalog:"));
    assert_eq!(
        prepared
            .updated_catalogs
            .get("default")
            .and_then(|catalog| catalog.get("foo"))
            .map(String::as_str),
        Some("2.0.0"),
    );
    assert_eq!(
        prepared
            .catalogs_override
            .as_ref()
            .and_then(|catalogs| catalogs.get("default"))
            .and_then(|catalog| catalog.get("foo"))
            .map(String::as_str),
        Some("2.0.0"),
    );
}
