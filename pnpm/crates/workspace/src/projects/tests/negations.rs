use super::{
    FindWorkspaceProjectsOpts, TempDir, find_project_names, find_workspace_projects, fs,
    make_project,
};

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
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new().unwrap();
    make_project(root.path(), ".", "root");
    make_project(root.path(), "generated/child", "excluded");
    let generated = root.path().join("generated");
    let permissions = fs::metadata(&generated).unwrap().permissions();
    fs::set_permissions(&generated, fs::Permissions::from_mode(0o000)).unwrap();
    let projects = find_workspace_projects(
        root.path(),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["**".to_string(), "!generated/**".to_string()]),
            ..Default::default()
        },
    );
    fs::set_permissions(&generated, permissions).unwrap();
    assert_eq!(projects.unwrap().len(), 1);
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
