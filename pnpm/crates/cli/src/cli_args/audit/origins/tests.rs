use super::{
    Origin,
    shared_versions,
};
use pnpm_lockfile::PackageKey;
use pretty_assertions::assert_eq;

fn key(text: &str) -> PackageKey {
    text.parse().expect("package key")
}

fn labels(labels: &[&str]) -> Vec<String> {
    labels
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn renders_the_chain_of_a_subdependency() {
    let origin = Origin { projects: Vec::new(), via: labels(&["request", "har"]) };
    assert_eq!(origin.render(), "via request > har");
}

#[test]
fn shortens_a_long_chain_to_its_ends() {
    let origin = Origin { projects: Vec::new(), via: labels(&["a", "b", "c", "d", "e", "f"]) };
    assert_eq!(origin.render(), "via a > ... > e > f");
    let origin = Origin { projects: Vec::new(), via: labels(&["a", "b", "c", "d"]) };
    assert_eq!(origin.render(), "via a > b > c > d");
}

#[test]
fn renders_the_first_projects_and_counts_the_rest() {
    let origin = Origin {
        projects: labels(&[".", "packages/a", "packages/b", "packages/c"]),
        via: Vec::new(),
    };
    assert_eq!(origin.render(), "., packages/a, packages/b and 1 more");
}

#[test]
fn rendering_strips_control_characters_from_lockfile_labels() {
    let origin = Origin { projects: labels(&["packages/\u{1b}[31ma"]), via: Vec::new() };
    assert_eq!(origin.render(), "packages/[31ma");
}

#[test]
fn a_version_installed_from_two_registries_is_shared() {
    let installed = [
        (key("foo@1.0.0"), "1.0.0".to_string()),
        (key("foo@work:1.0.0"), "1.0.0".to_string()),
        (key("bar@1.0.0"), "1.0.0".to_string()),
        (key("bar@1.0.0"), "1.0.0".to_string()),
    ];
    assert_eq!(
        shared_versions(installed.into_iter()),
        [("foo".to_string(), "1.0.0".to_string())].into(),
    );
}
