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
