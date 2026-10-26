use super::workspace_declared_specifier;
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::{WorkspacePackage, WorkspacePackages};
use serde_json::{Value, json};
use std::path::PathBuf;

fn workspace_with_dependency(
    group: DependencyGroup,
    name: &str,
    specifier: &str,
) -> WorkspacePackages {
    let mut manifest = json!({"name": "other", "version": "1.0.0"});
    let group_name: &str = group.into();
    manifest[group_name][name] = Value::String(specifier.to_string());
    WorkspacePackages::from([(
        "other".to_string(),
        [("1.0.0".to_string(), WorkspacePackage { root_dir: PathBuf::from("other"), manifest })]
            .into(),
    )])
}

#[test]
fn finds_a_dependency_range_in_another_workspace_package() {
    let workspace = workspace_with_dependency(DependencyGroup::Dev, "foo", "~2.4");

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), Some("~2.4".to_string()));
}

#[test]
fn does_not_find_a_different_dependency_name() {
    let workspace = workspace_with_dependency(DependencyGroup::Prod, "bar", "^3.0.0");

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), None);
}
