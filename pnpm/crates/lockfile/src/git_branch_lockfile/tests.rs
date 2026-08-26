use crate::Lockfile;
use std::fs;
use tempfile::TempDir;

#[test]
fn a_branch_name_is_lowercased_and_stripped_of_path_separators() {
    assert_eq!(Lockfile::git_branch_file_name("main"), "pnpm-lock.main.yaml");
    assert_eq!(Lockfile::git_branch_file_name("a/b/c"), "pnpm-lock.a!b!c.yaml");
    assert_eq!(Lockfile::git_branch_file_name("aBc"), "pnpm-lock.abc.yaml");
    assert_eq!(Lockfile::git_branch_file_name("feat/über"), "pnpm-lock.feat!!ber.yaml");
}

#[test]
fn the_branch_file_matcher_requires_literal_dots_and_a_branch_segment() {
    for name in ["pnpm-lock.main.yaml", "pnpm-lock.feature.x.yaml"] {
        assert!(Lockfile::is_git_branch_file_name(name), "{name} is a branch lockfile");
    }
    for name in
        ["pnpm-lock.yaml", "pnpm-lock..yaml", "pnpm-lock-main-yaml", "my-pnpm-lock.main.yaml"]
    {
        assert!(!Lockfile::is_git_branch_file_name(name), "{name} is not a branch lockfile");
    }
}

#[test]
fn scanning_lists_only_the_branch_lockfiles_and_cleaning_removes_exactly_those() {
    let dir = TempDir::new().unwrap();
    for name in [
        "pnpm-lock.main.yaml",
        "pnpm-lock.feature.x.yaml",
        "pnpm-lock.yaml",
        "pnpm-lock-main-yaml",
        "my-pnpm-lock.main.yaml",
        "README.md",
    ] {
        fs::write(dir.path().join(name), "").unwrap();
    }

    let found: Vec<String> = Lockfile::git_branch_lockfiles(dir.path())
        .unwrap()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(found, ["pnpm-lock.feature.x.yaml", "pnpm-lock.main.yaml"]);

    Lockfile::clean_git_branch_lockfiles(dir.path()).unwrap();
    let mut left: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        ["README.md", "my-pnpm-lock.main.yaml", "pnpm-lock-main-yaml", "pnpm-lock.yaml"],
    );
}

#[test]
fn scanning_a_missing_directory_finds_nothing() {
    let dir = TempDir::new().unwrap();
    assert!(Lockfile::git_branch_lockfiles(&dir.path().join("absent")).unwrap().is_empty());
}
