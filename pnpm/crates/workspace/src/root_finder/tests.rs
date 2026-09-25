use super::{
    BadWorkspaceManifestNameError, FindWorkspaceDirError, INVALID_WORKSPACE_MANIFEST_FILENAMES,
    WORKSPACE_DIR_ENV_VAR, WORKSPACE_DIR_ENV_VAR_LOWER, find_workspace_dir,
    find_workspace_dir_from_env_with,
};
use crate::{WORKSPACE_MANIFEST_FILENAME, api::EnvVarOs};
use pretty_assertions::assert_eq;
use std::{ffi::OsString, fs};
use tempfile::TempDir;

#[test]
fn finds_workspace_dir_at_start() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "packages:\n  - pkgs/*\n").unwrap();
    let found = find_workspace_dir(tmp.path()).unwrap();
    assert_eq!(found.as_deref(), Some(tmp.path()));
}

#[test]
fn finds_workspace_dir_in_ancestor() {
    let tmp = TempDir::new().unwrap();
    let nested = tmp.path().join("packages").join("a");
    fs::create_dir_all(&nested).unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "packages:\n  - packages/*\n").unwrap();
    let found = find_workspace_dir(&nested).unwrap();
    assert_eq!(found.as_deref(), Some(tmp.path()));
}

#[test]
fn returns_none_when_no_manifest() {
    let tmp = TempDir::new().unwrap();
    let found = find_workspace_dir(tmp.path()).unwrap();
    assert_eq!(found, None);
}

#[test]
fn rejects_invalid_filenames() {
    for bad in INVALID_WORKSPACE_MANIFEST_FILENAMES {
        let tmp = TempDir::new().unwrap();
        let bad_path = tmp.path().join(bad);
        fs::write(&bad_path, "packages: [a]\n").unwrap();
        let err = find_workspace_dir(tmp.path()).unwrap_err();
        match err {
            FindWorkspaceDirError::BadName(BadWorkspaceManifestNameError { path }) => {
                assert_eq!(path, bad_path, "bad variant: {bad}");
            }
            other => panic!("unexpected error for {bad}: {other:?}"),
        }
    }
}

/// When both the correct file and a misnamed variant are present,
/// the correct one wins — upstream's `findUp` returns the first match
/// in pattern order at each level, but the misnamed-variant check
/// applies only after the correct file is ruled out at the current
/// level. Same reasoning preserved here.
#[test]
fn correct_filename_wins_over_misnamed_sibling() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "packages:\n  - pkgs/*\n").unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yml"), "packages: [bad]\n").unwrap();
    let found = find_workspace_dir(tmp.path()).unwrap();
    assert_eq!(found.as_deref(), Some(tmp.path()));
}

/// An empty `NPM_CONFIG_WORKSPACE_DIR` must be treated as unset so
/// the upward walk takes over. Otherwise an exported-but-empty
/// variable would short-circuit discovery and force the install into
/// `PathBuf::from("")`. Mirrors upstream's truthy `if (workspaceDir)`
/// check.
///
/// `std::env::set_var` has documented UB when other threads access
/// the process environment concurrently (and Rust tests default to
/// multi-threaded). Routing the env lookup through the [`EnvVarOs`]
/// DI seam on [`find_workspace_dir_from_env_with`] lets this test
/// exercise the fall-through branch without touching the process
/// env at all.
#[test]
fn empty_env_var_is_treated_as_unset() {
    struct EnvWithEmptyWorkspaceDir;
    impl EnvVarOs for EnvWithEmptyWorkspaceDir {
        fn var_os(name: &str) -> Option<OsString> {
            (name == WORKSPACE_DIR_ENV_VAR).then(OsString::new)
        }
    }
    assert_eq!(
        find_workspace_dir_from_env_with::<EnvWithEmptyWorkspaceDir>(),
        None,
        "empty env var must fall through to the upward walk",
    );
}

#[test]
fn non_empty_env_var_resolves_verbatim() {
    struct EnvWithUppercaseWorkspaceDir;
    impl EnvVarOs for EnvWithUppercaseWorkspaceDir {
        fn var_os(name: &str) -> Option<OsString> {
            (name == WORKSPACE_DIR_ENV_VAR).then(|| OsString::from("/explicit/root"))
        }
    }
    assert_eq!(
        find_workspace_dir_from_env_with::<EnvWithUppercaseWorkspaceDir>(),
        Some(std::path::PathBuf::from("/explicit/root")),
    );
}

#[test]
fn lowercase_env_var_is_honored_as_fallback() {
    struct EnvWithLowercaseWorkspaceDir;
    impl EnvVarOs for EnvWithLowercaseWorkspaceDir {
        fn var_os(name: &str) -> Option<OsString> {
            (name == WORKSPACE_DIR_ENV_VAR_LOWER).then(|| OsString::from("/lowercase/root"))
        }
    }
    assert_eq!(
        find_workspace_dir_from_env_with::<EnvWithLowercaseWorkspaceDir>(),
        Some(std::path::PathBuf::from("/lowercase/root")),
    );
}

/// <https://github.com/pnpm/pnpm/issues/3561>
mod workspace_membership {
    use super::{TempDir, WORKSPACE_MANIFEST_FILENAME, find_workspace_dir, fs};
    use pretty_assertions::assert_eq;

    fn prepare_workspace(packages: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), packages).unwrap();
        for project in [".", "packages/pkg-1", "examples/example-1", "docs"] {
            let dir = tmp.path().join(project);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("package.json"), r#"{"name": "p", "version": "0.0.1"}"#).unwrap();
        }
        fs::create_dir_all(tmp.path().join("packages/pkg-1/src")).unwrap();
        tmp
    }

    #[test]
    fn excluded_project_finds_no_workspace_dir() {
        let tmp = prepare_workspace("packages:\n  - packages/**\n  - '!examples/**'\n");
        let found = find_workspace_dir(&tmp.path().join("examples/example-1")).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn unlisted_project_finds_no_workspace_dir() {
        let tmp = prepare_workspace("packages:\n  - packages/**\n");
        let found = find_workspace_dir(&tmp.path().join("docs")).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn listed_project_finds_the_workspace_dir() {
        let tmp = prepare_workspace("packages:\n  - packages/**\n");
        let found = find_workspace_dir(&tmp.path().join("packages/pkg-1")).unwrap();
        assert_eq!(found.as_deref(), Some(tmp.path()));
    }

    #[test]
    fn directory_without_a_manifest_finds_the_workspace_dir() {
        let tmp = prepare_workspace("packages:\n  - packages/**\n");
        let found = find_workspace_dir(&tmp.path().join("packages/pkg-1/src")).unwrap();
        assert_eq!(found.as_deref(), Some(tmp.path()));
    }

    #[test]
    fn workspace_root_finds_itself_though_no_pattern_lists_it() {
        let tmp = prepare_workspace("packages:\n  - packages/**\n");
        let found = find_workspace_dir(tmp.path()).unwrap();
        assert_eq!(found.as_deref(), Some(tmp.path()));
    }
}
