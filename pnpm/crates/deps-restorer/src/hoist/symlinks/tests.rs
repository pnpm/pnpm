use super::update_stale_hoist_symlink;

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
