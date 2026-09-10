use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, HostNoHome, LinkProbe, OsString, Path,
    PathBuf, assert_eq, fs, io, repo_on_branch, safe_host_var, tempdir,
};

#[test]
fn package_lock_is_the_lockfile_fallback() {
    let package_lock_only = tempdir().unwrap();
    fs::write(package_lock_only.path().join("pnpm-workspace.yaml"), "packageLock: false\n")
        .unwrap();
    let config = Config::new()
        .current::<HostNoHome>(package_lock_only.path())
        .expect("packageLock config loads");
    assert!(!config.package_lock);
    assert!(!config.lockfile);

    let explicit_lockfile = tempdir().unwrap();
    fs::write(
        explicit_lockfile.path().join("pnpm-workspace.yaml"),
        "packageLock: false\nlockfile: true\n",
    )
    .unwrap();
    let config = Config::new()
        .current::<HostNoHome>(explicit_lockfile.path())
        .expect("lockfile config loads");
    assert!(!config.package_lock);
    assert!(config.lockfile);
}

#[test]
pub fn gvs_disabled_or_extend_node_path_off_injects_no_resolution_env() {
    for yaml in [
        "enableGlobalVirtualStore: false\n",
        "enableGlobalVirtualStore: true\nextendNodePath: false\n",
    ] {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("pnpm-workspace.yaml"), yaml)
            .expect("write to pnpm-workspace.yaml");
        let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
        assert_eq!(config.extra_env.get("NODE_PATH"), None, "yaml: {yaml}");
        assert_eq!(config.extra_env.get("NODE_OPTIONS"), None, "yaml: {yaml}");
    }
}

#[test]
pub fn git_branch_lockfile_names_the_lockfile_after_the_current_branch() {
    let repo = repo_on_branch("ref: refs/heads/feat/Login\n");
    fs::write(repo.path().join("pnpm-workspace.yaml"), "gitBranchLockfile: true\n").unwrap();
    static REPO_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    REPO_DIR.set(repo.path().to_path_buf()).expect("set once");
    host_in_repo!(HostOnBranch);

    let config = Config::new().current::<HostOnBranch>(repo.path()).expect("yaml is valid");
    assert!(config.use_git_branch_lockfile);
    assert_eq!(config.git_branch_lockfile_name.as_deref(), Some("pnpm-lock.feat!login.yaml"));
    assert_eq!(config.wanted_lockfile_name(), "pnpm-lock.feat!login.yaml");
}

/// Merging collapses the per-branch lockfiles into the shared one, so the
/// install has to be reading and writing `pnpm-lock.yaml` while it does.
#[test]
pub fn merging_puts_the_install_back_on_the_shared_lockfile() {
    let repo = repo_on_branch("ref: refs/heads/main\n");
    fs::write(
        repo.path().join("pnpm-workspace.yaml"),
        "gitBranchLockfile: true\nmergeGitBranchLockfiles: true\n",
    )
    .unwrap();
    static REPO_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    REPO_DIR.set(repo.path().to_path_buf()).expect("set once");
    host_in_repo!(HostMerging);

    let config = Config::new().current::<HostMerging>(repo.path()).expect("yaml is valid");
    assert!(config.merge_git_branch_lockfiles);
    assert_eq!(config.git_branch_lockfile_name.as_deref(), Some("pnpm-lock.main.yaml"));
    assert_eq!(config.wanted_lockfile_name(), "pnpm-lock.yaml");
}

/// A detached HEAD has no branch to name a lockfile after, so the install
/// stays on `pnpm-lock.yaml` rather than inventing one.
#[test]
pub fn a_detached_head_leaves_the_install_on_the_shared_lockfile() {
    let repo = repo_on_branch("0123456789abcdef0123456789abcdef01234567\n");
    fs::write(repo.path().join("pnpm-workspace.yaml"), "gitBranchLockfile: true\n").unwrap();
    static REPO_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    REPO_DIR.set(repo.path().to_path_buf()).expect("set once");
    host_in_repo!(HostDetached);

    let config = Config::new().current::<HostDetached>(repo.path()).expect("yaml is valid");
    assert!(config.use_git_branch_lockfile);
    assert_eq!(config.git_branch_lockfile_name, None);
    assert_eq!(config.wanted_lockfile_name(), "pnpm-lock.yaml");
}
