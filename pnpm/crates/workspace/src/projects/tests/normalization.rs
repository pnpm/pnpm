use super::{find_project_names, make_project, make_yaml_project};
use crate::{FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, find_workspace_projects};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test]
fn normalizes_literal_and_glob_patterns() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_yaml_project(tmp.path(), "packages/beta", "beta");
    make_project(tmp.path(), "packages/.hidden", "hidden");
    make_project(tmp.path(), "projects/foo/bar/baz", "nested");

    for pattern in [
        "./packages/alpha",
        "././packages/alpha/",
        "./packages/./alpha",
        "packages/./alpha",
        "packages/alpha/.",
        "packages//alpha",
        ".//packages/alpha",
        "packages/missing/../alpha",
    ] {
        assert_eq!(
            find_project_names(tmp.path(), &[pattern]),
            vec!["root", "alpha"],
            "pattern: {pattern}",
        );
    }
    for pattern in [
        "./packages/*",
        "././packages/*",
        "./packages/**",
        "./packages/{alpha,beta}",
        "packages//./*",
        "packages/missing/../*",
    ] {
        assert_eq!(
            find_project_names(tmp.path(), &[pattern]),
            vec!["root", "alpha", "beta"],
            "pattern: {pattern}",
        );
    }
    assert_eq!(find_project_names(tmp.path(), &["./packages/.hidden"]), vec!["root", "hidden"]);
    assert_eq!(find_project_names(tmp.path(), &["./projects/foo/bar/baz"]), vec!["root", "nested"]);
    assert_eq!(find_project_names(tmp.path(), &["././", "packages/../"]), vec!["root"]);
    assert_eq!(
        find_project_names(tmp.path(), &["packages/alpha", "./packages/./alpha"]),
        vec!["root", "alpha"],
    );
}

#[test]
fn normalizes_negations_for_specialized_and_generic_patterns() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_yaml_project(tmp.path(), "packages/beta", "beta");

    for include in ["packages/*", "./packages/**"] {
        for exclude in [
            "!./packages/alpha",
            "!././packages/alpha/",
            "!packages/./alpha",
            "!./packages//alpha",
            "!packages/missing/../alpha",
        ] {
            assert_eq!(
                find_project_names(tmp.path(), &[include, exclude]),
                vec!["root", "beta"],
                "patterns: {include}, {exclude}",
            );
        }
    }
}

#[test]
fn normalizes_patterns_above_the_workspace_root() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), "workspace", "root");
    make_project(tmp.path(), "shared/alpha", "alpha");
    make_project(tmp.path(), "shared/beta", "beta");
    let workspace = tmp.path().join("workspace");

    for pattern in ["./../shared/*", "././../shared/*", "../shared/missing/../*"] {
        assert_eq!(
            find_project_names(&workspace, &[pattern, "!./../shared/./alpha"]),
            vec!["beta", "root"],
            "pattern: {pattern}",
        );
    }
}

#[test]
fn invalid_normalized_globs_report_the_original_pattern() {
    let tmp = TempDir::new().unwrap();
    for source in ["./packages//[", "!./packages//["] {
        let result = find_workspace_projects(
            tmp.path(),
            &FindWorkspaceProjectsOpts { patterns: Some(vec![source.to_string()]) },
        );
        let Err(FindWorkspaceProjectsError::InvalidGlob { pattern, .. }) = result else {
            panic!("expected an invalid glob for {source}");
        };
        assert_eq!(pattern, source);
    }
}

#[test]
fn negations_that_normalize_to_the_workspace_root_are_dropped() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");

    for exclude in ["!.", "!./", "!packages/.."] {
        assert_eq!(
            find_project_names(tmp.path(), &["packages/*", exclude]),
            vec!["root", "alpha"],
            "exclude: {exclude}",
        );
    }
}
