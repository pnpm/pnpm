use super::update_stale_hoist_symlink;

#[test]
fn distinguishes_non_links_from_ownership_read_failures() {
    assert!(super::is_non_link_read_error(&std::io::Error::from(std::io::ErrorKind::InvalidInput)));
    assert!(!super::is_non_link_read_error(&std::io::Error::from(
        std::io::ErrorKind::PermissionDenied,
    )));
}

#[test]
fn concurrent_hoists_replace_the_same_stale_dependency_link() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("store");
    let old_target = store.join("old");
    let target = store.join("new");
    let modules = root.path().join("node_modules");
    std::fs::create_dir_all(&old_target).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(target.join("index.js"), "module.exports = 2").unwrap();

    for iteration in 0..100 {
        let link = modules.join(format!("dep-{iteration}"));
        pnpm_fs::symlink_dir(&old_target, &link).unwrap();
        let barrier = std::sync::Barrier::new(32);
        let results = std::thread::scope(|scope| {
            let worker = || {
                barrier.wait();
                update_stale_hoist_symlink(&target, &link, &store, &modules)
            };
            let mut workers = Vec::new();
            for _ in 0..32 {
                workers.push(scope.spawn(worker));
            }
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        for result in results {
            result.expect("concurrent hoists must reuse the winning replacement");
        }
        assert_eq!(std::fs::canonicalize(&link).unwrap(), std::fs::canonicalize(&target).unwrap());
        assert_eq!(std::fs::read_to_string(link.join("index.js")).unwrap(), "module.exports = 2");
    }
}

#[test]
fn accepts_only_a_replacement_link_to_the_requested_dependency() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let other = root.path().join("other");
    std::fs::create_dir(&target).unwrap();
    std::fs::create_dir(&other).unwrap();
    for (name, winner) in [("same", &target), ("different", &other)] {
        let link = root.path().join(name);
        pnpm_fs::symlink_dir(winner, &link).unwrap();
        let result = super::create_hoist_symlink(&target, &link);
        if winner == &target {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        }
        assert_eq!(std::fs::canonicalize(&link).unwrap(), std::fs::canonicalize(winner).unwrap());
    }
}

#[test]
fn preserves_a_directory_created_by_another_writer() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    std::fs::create_dir(&target).unwrap();
    std::fs::create_dir(&link).unwrap();
    std::fs::write(link.join("sentinel"), "keep").unwrap();
    assert_eq!(
        super::create_hoist_symlink(&target, &link).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists,
    );
    assert!(!pnpm_fs::is_symlink_or_junction(&link).unwrap());
    assert_eq!(std::fs::read_to_string(link.join("sentinel")).unwrap(), "keep");
}

#[cfg(windows)]
#[test]
fn rechecks_a_link_after_a_reparse_point_read_race() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    std::fs::create_dir(&target).unwrap();
    junction::create(&target, &link).unwrap();
    let not_a_reparse_point = std::io::Error::from_raw_os_error(4390);
    assert!(super::should_retry_hoist_link_read(&link, &not_a_reparse_point));
    pnpm_fs::remove_symlink_dir(&link).unwrap();
    assert!(super::should_retry_hoist_link_read(&link, &not_a_reparse_point));
    // An empty directory is what a junction looks like before its reparse
    // point is set.
    std::fs::create_dir(&link).unwrap();
    assert!(super::should_retry_hoist_link_read(&link, &not_a_reparse_point));
    std::fs::write(link.join("sentinel"), "keep").unwrap();
    assert!(!super::should_retry_hoist_link_read(&link, &not_a_reparse_point));
}

#[cfg(windows)]
#[test]
fn waits_for_a_junction_another_hoist_is_creating() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    std::fs::create_dir(&target).unwrap();
    std::fs::create_dir(&link).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            std::fs::remove_dir(&link).unwrap();
            // The retrying hoist may create its link first once the directory
            // is gone. Either link points at `target`.
            let _ = junction::create(&target, &link);
        });
        super::create_hoist_symlink(&target, &link).unwrap();
    });
    assert_eq!(std::fs::canonicalize(&link).unwrap(), std::fs::canonicalize(&target).unwrap());
}

#[test]
fn recreates_a_link_removed_before_ownership_inspection() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    std::fs::create_dir(&target).unwrap();
    update_stale_hoist_symlink(&target, &link, root.path(), root.path()).unwrap();
    assert_eq!(std::fs::canonicalize(link).unwrap(), std::fs::canonicalize(target).unwrap());
}

#[cfg(windows)]
#[test]
fn rechecks_a_junction_completed_after_its_metadata_was_read() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("index.js"), "module.exports = 2").unwrap();
    std::fs::create_dir(&link).unwrap();
    let in_progress = std::fs::symlink_metadata(&link).unwrap();
    std::fs::remove_dir(&link).unwrap();
    junction::create(&target, &link).unwrap();
    assert!(super::may_be_junction_in_creation(&link, &in_progress));
}

#[cfg(windows)]
#[test]
fn treats_a_directory_its_creator_holds_exclusively_as_a_junction_in_creation() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let root = tempfile::tempdir().unwrap();
    let link = root.path().join("link");
    std::fs::create_dir(&link).unwrap();
    let in_progress = std::fs::symlink_metadata(&link).unwrap();
    let _creator = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(&link)
        .unwrap();
    assert!(super::may_be_junction_in_creation(&link, &in_progress));
}
