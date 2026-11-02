use super::{VersionlessSelector, workspace_declared_specifier};
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::{WorkspacePackage, WorkspacePackages};
use serde_json::{Value, json};
use std::path::PathBuf;

fn workspace_with_dependencies(projects: &[(DependencyGroup, &str, &str)]) -> WorkspacePackages {
    projects
        .iter()
        .enumerate()
        .map(|(index, (group, name, specifier))| {
            let project_name = format!("project-{index}");
            let mut manifest = json!({"name": project_name, "version": "1.0.0"});
            let group_name: &str = (*group).into();
            manifest[group_name][*name] = Value::String(specifier.to_string());
            let package = WorkspacePackage { root_dir: PathBuf::from(&project_name), manifest };
            (project_name, [("1.0.0".to_string(), package)].into())
        })
        .collect()
}

#[test]
fn finds_a_dependency_range_in_another_workspace_package() {
    let workspace = workspace_with_dependencies(&[(DependencyGroup::Dev, "foo", "~2.4")]);

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), Some("~2.4".to_string()));
}

#[test]
fn does_not_find_a_different_dependency_name() {
    let workspace = workspace_with_dependencies(&[(DependencyGroup::Prod, "bar", "^3.0.0")]);

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), None);
}

#[test]
fn finds_a_range_every_workspace_package_agrees_on() {
    let workspace = workspace_with_dependencies(&[
        (DependencyGroup::Prod, "foo", "^1.0.0"),
        (DependencyGroup::Dev, "foo", "^1.0.0"),
    ]);

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), Some("^1.0.0".to_string()));
}

#[test]
fn ignores_ranges_the_workspace_packages_disagree_on() {
    let workspace = workspace_with_dependencies(&[
        (DependencyGroup::Prod, "foo", "^1.0.0"),
        (DependencyGroup::Prod, "foo", "^2.0.0"),
    ]);

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), None);
}

#[test]
fn ignores_a_dependency_any_workspace_package_declares_with_a_protocol() {
    let workspace = workspace_with_dependencies(&[
        (DependencyGroup::Prod, "foo", "workspace:*"),
        (DependencyGroup::Prod, "foo", "^1.0.0"),
    ]);

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), None);
}

#[test]
fn ignores_peer_dependencies() {
    let workspace = workspace_with_dependencies(&[(DependencyGroup::Peer, "foo", "^1.0.0")]);

    assert_eq!(workspace_declared_specifier("foo", Some(&workspace)), None);
}

#[test]
fn keys_a_versionless_selector_by_its_manifest_name() {
    let registry = |name: &str| Some(VersionlessSelector::Registry(name.to_string()));
    assert_eq!(VersionlessSelector::parse("foo"), registry("foo"));
    assert_eq!(VersionlessSelector::parse("npm:foo"), registry("foo"));
    assert_eq!(
        VersionlessSelector::parse("alias@npm:foo"),
        Some(VersionlessSelector::NpmAlias("alias".to_string())),
    );
    assert_eq!(VersionlessSelector::parse("foo@^1.0.0"), None);
    assert_eq!(VersionlessSelector::parse("alias@npm:foo@^1.0.0"), None);
}
