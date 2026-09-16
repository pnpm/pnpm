use super::{exec_git, make_bare_repo_with_prepare_script};
use crate::{cache_checkout_bundles, checkout_cached_bundles};
use std::fs;

#[test]
fn cached_bundles_restore_committed_files_without_cache_configuration_or_hooks() {
    let root = tempfile::tempdir().unwrap();
    let (_, commit) = make_bare_repo_with_prepare_script(root.path(), "true");
    exec_git(&["-c", "tag.gpgSign=false", "tag", "v1"], Some(&root.path().join("work"))).unwrap();
    let cache = root.path().join("cache");
    cache_checkout_bundles(&root.path().join("work"), &cache).unwrap();
    fs::create_dir_all(cache.join(".git/hooks")).unwrap();
    fs::write(cache.join(".git/config"), "[core]\nfsmonitor = touch compromised\n").unwrap();
    fs::write(cache.join(".git/hooks/post-checkout"), "#!/bin/sh\ntouch compromised\n").unwrap();
    fs::write(cache.join("index.js"), "module.exports = 'poisoned';\n").unwrap();
    fs::write(cache.join("untracked-backend.py"), "raise RuntimeError('poisoned')\n").unwrap();
    let restored = root.path().join("restored");
    assert_eq!(checkout_cached_bundles(&cache, &commit, &restored).unwrap(), commit);
    assert_eq!(fs::read_to_string(restored.join("index.js")).unwrap(), "module.exports = 'src';\n");
    assert!(!restored.join("untracked-backend.py").exists());
    assert!(!restored.join("compromised").exists());
    assert_eq!(
        exec_git(&["describe", "--tags", "--exact-match"], Some(&restored)).unwrap().trim(),
        "v1",
    );
    assert_eq!(exec_git(&["status", "--porcelain"], Some(&restored)).unwrap(), "");
}

#[test]
fn invalid_cached_bundle_cannot_restore_a_locked_commit() {
    let root = tempfile::tempdir().unwrap();
    let (_, commit) = make_bare_repo_with_prepare_script(root.path(), "true");
    let cache = root.path().join("cache");
    cache_checkout_bundles(&root.path().join("work"), &cache).unwrap();
    fs::write(cache.join("repository.bundle"), "poisoned bundle").unwrap();
    assert!(checkout_cached_bundles(&cache, &commit, &root.path().join("restored")).is_err());
}
