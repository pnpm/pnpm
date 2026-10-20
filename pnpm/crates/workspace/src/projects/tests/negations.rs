use super::{TempDir, find_project_names, fs, make_project};

#[test]
fn subtree_exclusions_preserve_directory_only_exclusions_and_explicit_hidden_includes() {
    let root = TempDir::new().unwrap();
    make_project(root.path(), ".", "root");
    make_project(root.path(), "packages/generated", "generated");
    make_project(root.path(), "packages/generated/child", "child");
    make_project(root.path(), "packages/.hidden/tool", "hidden");
    make_project(root.path(), "packages/keep", "keep");
    assert_eq!(
        find_project_names(root.path(), &["**", "packages/.hidden/**", "!packages/generated"]),
        vec!["root", "hidden", "child", "keep"],
    );
    assert_eq!(
        find_project_names(
            root.path(),
            &["**", "packages/.hidden/**", "!packages/generated/**", "!packages/.hidden/**"]
        ),
        vec!["root", "keep"],
    );
    assert_eq!(find_project_names(root.path(), &["**", "!**"]), vec!["root"]);
}

#[test]
fn subtree_exclusions_are_relative_to_workspace_even_when_walking_its_parent() {
    let root = TempDir::new().unwrap();
    let workspace = root.path().join("work[space]");
    make_project(root.path(), "work[space]", "root");
    make_project(root.path(), "work[space]/generated/child", "excluded");
    make_project(root.path(), "work[space]/packages/keep", "keep");
    make_project(root.path(), "generated/child", "outside");
    make_project(root.path(), "shared/drop/child", "drop");
    make_project(root.path(), "shared/keep", "shared");
    assert_eq!(
        find_project_names(&workspace, &["../**", "!generated/**", "!../shared/drop/**"]),
        vec!["outside", "shared", "root", "keep"],
    );
}

#[cfg(unix)]
#[test]
fn subtree_exclusions_follow_symlink_spelling_without_excluding_other_paths_to_the_target() {
    use std::os::unix::fs::symlink;
    let root = TempDir::new().unwrap();
    make_project(root.path(), ".", "root");
    make_project(root.path(), "generated/child", "child");
    symlink(root.path().join("generated"), root.path().join("linked")).unwrap();
    assert_eq!(find_project_names(root.path(), &["**", "!generated/**"]), vec!["root", "child"]);
    assert_eq!(find_project_names(root.path(), &["**", "!linked/**"]), vec!["root", "child"]);
}

#[cfg(unix)]
#[test]
fn subtree_exclusions_do_not_read_excluded_directories() {
    use super::{FindWorkspaceProjectsOpts, find_workspace_projects};
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new().unwrap();
    make_project(root.path(), ".", "root");
    make_project(root.path(), "generated/child", "excluded");
    make_project(root.path(), "packages/keep", "keep");
    make_project(root.path(), "packages/generated/child", "excluded-pkg");
    let generated = root.path().join("generated");
    let pkg_generated = root.path().join("packages/generated");
    let permissions = fs::metadata(&generated).unwrap().permissions();
    let pkg_permissions = fs::metadata(&pkg_generated).unwrap().permissions();
    fs::set_permissions(&generated, fs::Permissions::from_mode(0o000)).unwrap();
    fs::set_permissions(&pkg_generated, fs::Permissions::from_mode(0o000)).unwrap();
    let projects = std::panic::catch_unwind(|| {
        let root_glob = find_workspace_projects(
            root.path(),
            &FindWorkspaceProjectsOpts {
                patterns: Some(vec![
                    "**".to_string(),
                    "!generated/**".to_string(),
                    "!packages/generated/**".to_string(),
                ]),
                ..Default::default()
            },
        )?;
        let prefixed_glob = find_workspace_projects(
            root.path(),
            &FindWorkspaceProjectsOpts {
                patterns: Some(vec![
                    "packages/**".to_string(),
                    "!packages/generated/**".to_string(),
                ]),
                ..Default::default()
            },
        )?;
        Ok::<_, super::FindWorkspaceProjectsError>((root_glob.len(), prefixed_glob.len()))
    });
    fs::set_permissions(&generated, permissions).unwrap();
    fs::set_permissions(&pkg_generated, pkg_permissions).unwrap();
    let projects = projects.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert_eq!(projects.unwrap(), (2, 2));
}

#[test]
fn wildcard_exclusions_match_dot_directories_included_explicitly() {
    let root = TempDir::new().unwrap();
    make_project(root.path(), ".", "root");
    make_project(root.path(), "packages/.dev/tool", "excluded-dot");
    make_project(root.path(), "packages/.dev/keep", "keep-dot");
    assert_eq!(
        find_project_names(root.path(), &["packages/.dev/*", "!packages/*/tool"]),
        vec!["root", "keep-dot"],
    );
}

#[cfg(unix)]
#[test]
fn recursive_globs_stop_at_ancestor_symlink_cycles() {
    use std::os::unix::fs::symlink;
    let root = TempDir::new().unwrap();
    make_project(root.path(), ".", "root");
    make_project(root.path(), "packages/alpha", "alpha");
    symlink(root.path().join("packages"), root.path().join("packages/alpha/loop")).unwrap();
    assert_eq!(find_project_names(root.path(), &["packages/**"]), vec!["root", "alpha"]);
    assert_eq!(find_project_names(root.path(), &["packages/**/alpha"]), vec!["root", "alpha"]);
}

#[cfg(target_os = "linux")]
#[test]
fn subtree_exclusions_do_not_confuse_non_unicode_roots_with_replacement_characters() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let root = TempDir::new().unwrap();
    let workspace = root
        .path()
        .join(OsString::from_vec(b"work\xffspace".to_vec()));
    fs::create_dir_all(&workspace).unwrap();
    make_project(&workspace, ".", "root");
    make_project(&workspace, "generated/child", "excluded");
    make_project(root.path(), "work\u{fffd}space/generated/child", "keep");
    let mut names = find_project_names(&workspace, &["../**", "!generated/**"]);
    names.sort();
    assert_eq!(names, vec!["keep", "root"]);
}

#[test]
fn parent_relative_exclusions_do_not_hide_workspace_local_descendants() {
    let root = TempDir::new().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    make_project(&workspace, ".", "root");
    make_project(&workspace, "packages/keep", "keep");
    make_project(root.path(), "outside/child", "outside");
    assert_eq!(find_project_names(&workspace, &["../**", "!../**"]), vec!["root", "keep"]);
}
