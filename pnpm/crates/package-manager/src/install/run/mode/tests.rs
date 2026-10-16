use super::{InstallExecution, fetches_into_store};

fn execution(lockfile_only: bool, dry_run: bool) -> InstallExecution {
    InstallExecution {
        skip_runtimes: false,
        mutation: crate::ProjectMutation::InstallWorkspace,
        installs_only: true,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only,
        dry_run,
    }
}

#[test]
fn only_a_disabled_modules_dir_fetches_into_the_store() {
    // The effective lockfile-only came from the config alone.
    assert!(fetches_into_store(true, execution(false, false)));
    // `--lockfile-only` and `--dry-run` fetch nothing, modules dir or not.
    assert!(!fetches_into_store(true, execution(true, false)));
    assert!(!fetches_into_store(true, execution(false, true)));
    // A run that materializes `node_modules` fetches as part of that.
    assert!(!fetches_into_store(false, execution(false, false)));
}

#[cfg(unix)]
#[test]
fn rollback_restores_a_symlinked_include_through_the_link() {
    use super::WorkspaceManifestRollbackGuard;

    let dir = tempfile::tempdir().expect("temp dir");
    let shared = dir.path().join("shared.yaml");
    let link = dir.path().join("policy.yaml");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "include:\n  ./policy.yaml: '*'\n")
        .expect("write root");
    std::fs::write(&shared, "allowBuilds:\n  esbuild: true\n").expect("write include");
    std::os::unix::fs::symlink(&shared, &link).expect("symlink include");

    let snapshot = WorkspaceManifestRollbackGuard::snapshot(dir.path());
    std::fs::write(&shared, "").expect("prune include");
    drop(WorkspaceManifestRollbackGuard::new(snapshot).expect("pruned file"));

    assert!(
        link.symlink_metadata()
            .expect("link")
            .file_type()
            .is_symlink(),
    );
    assert_eq!(
        std::fs::read_to_string(&shared).expect("read include"),
        "allowBuilds:\n  esbuild: true\n",
    );
}
