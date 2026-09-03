use super::{
    FindWorkspaceProjectsError, FindWorkspaceProjectsOpts, SpecializedPattern,
    find_workspace_projects, specialized_pattern,
};
use pretty_assertions::assert_eq;
use std::{fs, io::ErrorKind, path::Path};
use tempfile::TempDir;

fn make_project(root: &std::path::Path, rel: &str, name: &str) {
    let dir = root.join(rel);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.json"), format!(r#"{{"name": "{name}", "version": "0.0.1"}}"#))
        .unwrap();
}

fn make_yaml_project(root: &std::path::Path, rel: &str, name: &str) {
    let dir = root.join(rel);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.yaml"), format!("name: {name}\nversion: 0.0.1\n")).unwrap();
}

fn find_project_names(root: &Path, patterns: &[&str]) -> Vec<String> {
    find_workspace_projects(
        root,
        &FindWorkspaceProjectsOpts {
            patterns: Some(patterns.iter().map(|pattern| (*pattern).to_string()).collect()),
        },
    )
    .unwrap()
    .iter()
    .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
    .collect()
}

#[test]
fn recognizes_specialized_workspace_patterns() {
    assert_eq!(
        specialized_pattern("packages/alpha"),
        Some(SpecializedPattern::Literal("packages/alpha")),
    );
    assert_eq!(
        specialized_pattern("packages/alpha/"),
        Some(SpecializedPattern::Literal("packages/alpha")),
    );
    assert_eq!(specialized_pattern("packages/*"), Some(SpecializedPattern::ChildrenOf("packages")));
    assert_eq!(
        specialized_pattern("packages/*/"),
        Some(SpecializedPattern::ChildrenOf("packages")),
    );
}

#[test]
fn leaves_other_workspace_patterns_to_the_generic_walk() {
    for pattern in [
        "../shared",
        "/shared",
        "C:/shared",
        "packages/**",
        "packages/{alpha,beta}",
        "packages/$",
        "packages/<alpha>",
        "packages/(alpha,beta)",
        "packages/./alpha",
        "../shared/*",
        "/shared/*",
        "C:/shared/*",
        "packages/*/*",
        "packages/{alpha,beta}/*",
        "packages/$/*",
        "packages/<alpha>/*",
        "packages/(alpha,beta)/*",
        "packages/./*",
    ] {
        assert_eq!(specialized_pattern(pattern), None, "pattern: {pattern}");
    }
}

#[test]
fn expands_packages_glob() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_project(tmp.path(), "packages/beta", "beta");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string(), "beta".to_string()]);
}

#[test]
fn terminal_star_does_not_match_dotted_directories() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_project(tmp.path(), "packages/.cache", "cached");

    let names = find_project_names(tmp.path(), &["packages/*"]);
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string()]);
}

#[test]
fn recursive_wildcard_does_not_match_dotted_directories() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "nested/plain/deep", "plain");
    make_project(tmp.path(), "nested/.hidden/deep", "hidden");

    let names = find_project_names(tmp.path(), &["nested/**"]);
    assert_eq!(names, vec!["root".to_string(), "plain".to_string()]);
}

#[test]
fn non_terminal_star_does_not_match_dotted_directories() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha/lib", "alpha-lib");
    make_project(tmp.path(), "packages/.cache/lib", "cached-lib");

    let names = find_project_names(tmp.path(), &["packages/*/lib"]);
    assert_eq!(names, vec!["root".to_string(), "alpha-lib".to_string()]);
}

/// A wildcard must not reach a dot-prefixed directory, but a pattern that
/// names one must.
#[test]
fn a_pattern_naming_a_dotted_directory_still_matches_it() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), ".config/alpha", "alpha");
    make_project(tmp.path(), "packages/.cache", "cached");

    assert_eq!(
        find_project_names(tmp.path(), &["packages/.cache"]),
        vec!["root".to_string(), "cached".to_string()],
    );
    assert_eq!(
        find_project_names(tmp.path(), &[".config/*"]),
        vec!["root".to_string(), "alpha".to_string()],
    );
    assert_eq!(
        find_project_names(tmp.path(), &["packages/.*"]),
        vec!["root".to_string(), "cached".to_string()],
    );
}

#[test]
fn terminal_star_matches_only_immediate_child_directories() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_project(tmp.path(), "packages/group/beta", "beta");
    fs::write(tmp.path().join("packages/not-a-directory"), "text").unwrap();

    let names = find_project_names(tmp.path(), &["packages/*"]);
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string()]);
}

#[test]
fn terminal_star_applies_built_in_ignores() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "node_modules/alpha", "alpha");
    make_project(tmp.path(), "bower_components/beta", "beta");

    let names = find_project_names(tmp.path(), &["node_modules/*", "bower_components/*"]);
    assert_eq!(names, vec!["root".to_string()]);
}

#[test]
fn terminal_star_applies_user_negations() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_project(tmp.path(), "packages/beta", "beta");

    let names = find_project_names(tmp.path(), &["packages/*", "!packages/alpha"]);
    assert_eq!(names, vec!["root".to_string(), "beta".to_string()]);
}

#[cfg(unix)]
#[test]
fn terminal_star_does_not_follow_symlinked_child_directories() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "linked-target", "linked");
    fs::create_dir_all(tmp.path().join("packages")).unwrap();
    symlink(tmp.path().join("linked-target"), tmp.path().join("packages/linked")).unwrap();

    let names = find_project_names(tmp.path(), &["packages/*"]);
    assert_eq!(names, vec!["root".to_string()]);
}

#[test]
fn wax_metacharacters_use_generic_semantics() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "products/$/pkg", "dollar-pkg");
    make_project(tmp.path(), "products/alpha/pkg", "alpha-pkg");
    make_project(tmp.path(), "products/beta/pkg", "beta-pkg");
    make_project(tmp.path(), "products/beta/other", "beta-other");

    let mut names = find_project_names(tmp.path(), &["products/$/pkg"]);
    names.sort();
    assert_eq!(
        names,
        vec![
            "alpha-pkg".to_string(),
            "beta-pkg".to_string(),
            "dollar-pkg".to_string(),
            "root".to_string(),
        ],
    );
}

#[test]
fn expands_packages_glob_to_package_yaml() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_yaml_project(tmp.path(), "packages/alpha", "alpha");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string()]);
}

#[test]
fn direct_package_patterns_support_both_manifest_formats() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_yaml_project(tmp.path(), "packages/beta", "beta");

    let names = find_project_names(tmp.path(), &["packages/alpha", "packages/beta"]);
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string(), "beta".to_string()]);
}

#[test]
fn missing_direct_package_pattern_matches_nothing() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");

    let names = find_project_names(tmp.path(), &["packages/alpha"]);
    assert_eq!(names, vec!["root".to_string()]);
}

#[test]
fn direct_package_pattern_applies_user_negations() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");

    let names = find_project_names(tmp.path(), &["packages/alpha", "!packages/alpha"]);
    assert_eq!(names, vec!["root".to_string()]);
}

#[cfg(unix)]
#[test]
fn direct_package_pattern_follows_symlinked_directory() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "linked-target", "linked");
    fs::create_dir_all(tmp.path().join("packages")).unwrap();
    symlink(tmp.path().join("linked-target"), tmp.path().join("packages/linked")).unwrap();

    let names = find_project_names(tmp.path(), &["packages/linked"]);
    assert_eq!(names, vec!["root".to_string(), "linked".to_string()]);
}

#[test]
fn package_json_wins_when_both_manifest_files_exist() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "json-alpha");
    make_yaml_project(tmp.path(), "packages/alpha", "yaml-alpha");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "json-alpha".to_string()]);
}

#[test]
fn always_includes_workspace_root() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "apps/web", "web");

    // Patterns deliberately do NOT cover the root. Upstream still
    // surfaces it (https://github.com/pnpm/pnpm/issues/1986).
    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["apps/*".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "web".to_string()]);
}

#[test]
fn filters_node_modules() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "node_modules/foo", "foo");
    make_project(tmp.path(), "packages/real", "real");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["**".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert!(
        !names.contains(&"foo".to_string()),
        "node_modules contents must not surface as workspace projects: {names:?}",
    );
    assert!(
        names.contains(&"real".to_string()),
        "expected the `real` project to be enumerated; got {names:?}",
    );
}

#[test]
fn dedupes_overlapping_patterns() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["packages/*".to_string(), "**".to_string()]),
        },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string()]);
}

#[test]
fn default_patterns_when_packages_omitted() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "apps/web", "web");

    let projects =
        find_workspace_projects(tmp.path(), &FindWorkspaceProjectsOpts { patterns: None }).unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "web".to_string()]);
}

#[test]
fn negation_pattern_excludes_matching_projects() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "components/component-1", "component-1");
    make_project(tmp.path(), "components/component-2", "component-2");
    make_project(tmp.path(), "libs/foo", "foo");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["**".to_string(), "!libs/**".to_string()]),
        },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert!(
        !names.contains(&"foo".to_string()),
        "libs/foo must be excluded by `!libs/**`: {names:?}",
    );
    assert!(
        names.contains(&"component-1".to_string()) && names.contains(&"component-2".to_string()),
        "components must still be included: {names:?}",
    );
}

#[test]
fn negation_pattern_with_leading_slash_is_noop() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "components/component-1", "component-1");
    make_project(tmp.path(), "components/component-2", "component-2");
    make_project(tmp.path(), "libs/foo", "foo");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["**".to_string(), "!/libs/**".to_string()]),
        },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert!(
        names.contains(&"foo".to_string()),
        "`!/libs/**` must be a no-op; expected libs/foo to be included: {names:?}",
    );
}

/// `packages: []` (explicit empty array) is *not* the same as
/// omitted: it means "enumerate only the workspace root project,"
/// matching upstream's `opts.patterns ?? defaults` where `[]` is a
/// truthy value that survives the nullish-coalesce. Without this,
/// `packages: []` would silently fall back to `['.', '**']` and
/// recurse through the whole tree.
#[test]
fn empty_patterns_array_enumerates_root_only() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "apps/web", "web");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(Vec::new()) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string()]);
}

/// A pattern whose directory does not exist matches nothing instead of
/// aborting the enumeration, so a workspace that declares `packages/*`
/// before creating `packages/` still resolves its root project
/// (pnpm/pnpm#13296).
#[test]
fn missing_pattern_directory_matches_nothing() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["packages/*".to_string(), "apps/**".to_string()]),
        },
    )
    .expect("a missing pattern directory is not an error");

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string()]);
}

/// The absorbed kind is `NotFound` only: any other walk failure is real
/// and must not be mistaken for "no matches", with its `io::ErrorKind`
/// intact so the decision can be made at all.
///
/// A regular file where the pattern expects a directory, rather than a
/// `0o000` directory — the walk then fails for a reason no privilege level
/// can bypass, so the fixture holds even when the tests run as root.
#[test]
fn non_notfound_walk_failure_still_errors() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    fs::write(tmp.path().join("packages"), "not a directory").unwrap();

    let result = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*".to_string()]) },
    );

    // `expect_err` would need `Project: Debug`, which it deliberately is not.
    let Err(FindWorkspaceProjectsError::Walk { source, .. }) = result else {
        panic!("a non-NotFound walk failure must surface as Walk, not an empty match");
    };
    dbg!(&source);
    assert_ne!(
        source.kind(),
        ErrorKind::NotFound,
        "the walk error's kind must survive the conversion, or the skip cannot be decided",
    );
}

#[test]
fn non_notfound_walk_failure_still_errors_on_the_generic_path() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    fs::write(tmp.path().join("packages"), "not a directory").unwrap();

    // A non-terminal star keeps this off the specialized paths, which reach
    // the filesystem through their own enumeration.
    let result = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*/lib".to_string()]) },
    );

    let Err(FindWorkspaceProjectsError::Walk { source, .. }) = result else {
        panic!("a non-NotFound walk failure must surface as Walk, not an empty match");
    };
    dbg!(&source);
    assert_ne!(source.kind(), ErrorKind::NotFound);
}

/// A `packages:` entry may point outside the workspace root, and the
/// declared project has to be enumerated all the same, or the install
/// later rejects its lockfile importer as untrusted (pnpm/pnpm#13880).
#[test]
fn discovers_projects_declared_above_the_workspace_root() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), "shared", "shared-package");
    make_project(tmp.path(), "vendor/libs/parser", "parser");
    make_project(tmp.path(), "workspace", "workspace-root");
    make_project(tmp.path(), "workspace/app", "example-app");

    let projects = find_workspace_projects(
        &tmp.path().join("workspace"),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec![
                "app".to_string(),
                "../shared".to_string(),
                "../vendor/libs/*".to_string(),
            ]),
        },
    )
    .unwrap();

    let mut names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "example-app".to_string(),
            "parser".to_string(),
            "shared-package".to_string(),
            "workspace-root".to_string(),
        ],
    );
}

/// A negation is written relative to the workspace root whichever
/// ancestor the include it narrows walks from.
#[test]
fn negation_pattern_excludes_a_project_above_the_workspace_root() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), "shared/keep", "keep");
    make_project(tmp.path(), "shared/drop", "drop");
    make_project(tmp.path(), "workspace", "workspace-root");

    let projects = find_workspace_projects(
        &tmp.path().join("workspace"),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["../shared/*".to_string(), "!../shared/drop".to_string()]),
        },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert!(
        !names.contains(&"drop".to_string()),
        "`!../shared/drop` must exclude the project it names: {names:?}",
    );
    assert!(
        names.contains(&"keep".to_string()),
        "expected the `keep` project to be enumerated; got {names:?}",
    );
}

/// A pattern may climb past the filesystem root, which has no ancestor to
/// walk from. It matches nothing rather than falling back to the workspace
/// root and enumerating whatever happens to sit there.
#[test]
fn pattern_climbing_past_the_filesystem_root_matches_nothing() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), "shared", "shared-package");
    make_project(tmp.path(), "workspace", "workspace-root");
    // Discovered only if the overflowed pattern is silently rebased onto
    // the workspace root.
    make_project(tmp.path(), "workspace/app", "example-app");

    let workspace_root = tmp.path().join("workspace");
    // One `../` per component plus a spare, so the climb runs out of
    // ancestors wherever the temporary directory lives.
    let overflow = "../".repeat(workspace_root.components().count() + 1);

    let projects = find_workspace_projects(
        &workspace_root,
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec![format!("{overflow}*"), format!("{overflow}shared")]),
        },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        vec!["workspace-root".to_string()],
        "a pattern that overflows the filesystem root must contribute no project",
    );
}

/// Workspaces do contain manifests written with a leading UTF-8 BOM —
/// Vite ships one as the `utf8-bom-package` fixture — and discovery must
/// enumerate them rather than failing the whole walk.
#[test]
fn discovers_a_project_whose_manifest_starts_with_a_utf8_bom() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    let dir = tmp.path().join("packages/utf8-bom-package");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.json"), "\u{feff}{\"name\": \"bom\", \"version\": \"1.0.0\"}\n")
        .unwrap();

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "bom".to_string()]);
}

#[test]
fn a_named_dot_directory_does_not_exempt_later_wildcards() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/.cache/plain/lib", "plain-lib");
    make_project(tmp.path(), "packages/.cache/.hidden/lib", "hidden-lib");

    let names = find_project_names(tmp.path(), &["packages/.cache/*/lib"]);
    assert_eq!(names, vec!["root".to_string(), "plain-lib".to_string()]);
}

/// The exemption is positional, not by name: a wildcard must not match a dot
/// component just because the pattern names one spelled the same way.
#[test]
fn a_named_dot_directory_does_not_exempt_a_repeat_of_its_own_name() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/.cache/plain/lib", "plain-lib");
    make_project(tmp.path(), "packages/.cache/.cache/lib", "repeat-lib");

    let names = find_project_names(tmp.path(), &["packages/.cache/*/lib"]);
    assert_eq!(names, vec!["root".to_string(), "plain-lib".to_string()]);
}

#[test]
fn a_named_dot_directory_does_not_exempt_a_recursive_wildcard() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), ".config/plain/lib", "plain-lib");
    make_project(tmp.path(), ".config/.hidden/lib", "hidden-lib");

    let names = find_project_names(tmp.path(), &[".config/**"]);
    assert_eq!(names, vec!["root".to_string(), "plain-lib".to_string()]);
}

/// A manifest that fails to parse must fail the discovery, and with
/// several broken the reported one is a function of the project list —
/// the first in root order — not of read scheduling.
#[test]
fn a_malformed_manifest_fails_discovery_deterministically() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    // Component-wise joins, so the expected path below carries native
    // separators like the discovery walk's error message does.
    let broken = tmp.path().join("packages").join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("package.json"), "{ not json").unwrap();
    let broken_late = tmp.path().join("packages").join("zeta");
    fs::create_dir_all(&broken_late).unwrap();
    fs::write(broken_late.join("package.json"), "{ not json either").unwrap();

    let result = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*".to_string()]) },
    );

    // `expect_err` would need `Project: Debug`, which it deliberately is not.
    let Err(FindWorkspaceProjectsError::ReadManifest(source)) = result else {
        panic!("a malformed manifest must surface as ReadManifest, not be dropped");
    };
    let message = source.to_string();
    dbg!(&message);
    assert!(
        message.contains(&broken.join("package.json").display().to_string()),
        "the reported manifest must be the first broken project in root order",
    );
}

#[test]
fn an_invalid_glob_pattern_fails_before_any_walk() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    // Walking the *earlier* pattern would fail with `Walk`: `packages`
    // is a file, and the non-terminal star keeps the pattern off the
    // specialized paths. Errors are otherwise selected in pattern-list
    // order, so `InvalidGlob` winning proves the malformed pattern is
    // rejected by the up-front parse check, before any pattern walks.
    fs::write(tmp.path().join("packages"), "not a directory").unwrap();

    let result = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["packages/*/lib".to_string(), "nodes/[invalid".to_string()]),
        },
    );

    // `expect_err` would need `Project: Debug`, which it deliberately is not.
    let Err(FindWorkspaceProjectsError::InvalidGlob { pattern, .. }) = result else {
        panic!("an invalid glob must fail discovery before any walk");
    };
    assert_eq!(pattern, "nodes/[invalid");
}
