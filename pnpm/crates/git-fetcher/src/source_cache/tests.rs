use super::{GitSourceCache, GitSourceOptions};
use pnpm_fs::copy_dir_contents;
use pnpm_testing_utils::git_repo::GitRepoFixture;
use std::{
    fs,
    sync::{Arc, Barrier},
    thread,
};
use tempfile::tempdir;

#[test]
fn concurrent_and_sequential_consumers_share_a_checkout() {
    let root = tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "source");
    repo.write_file("package.json", r#"{"name":"source","version":"1.0.0"}"#);
    let commit = repo.commit("initial");
    let url = repo.file_url();
    let cache = GitSourceCache::default();
    #[cfg(unix)]
    let log = pnpm_testing_utils::git_repo::GitCommandLog::new(root.path());
    let opts = GitSourceOptions {
        repo: &url,
        commit: &commit,
        git_shallow_hosts: &[],
        #[cfg(unix)]
        git_bin: Some(&log.bin),
        #[cfg(not(unix))]
        git_bin: None,
    };
    let barrier = Barrier::new(8);
    let sources = thread::scope(|scope| {
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    cache.get(&opts).unwrap()
                })
            })
            .collect();
        tasks.into_iter().map(|task| task.join().unwrap()).collect::<Vec<_>>()
    });
    for source in &sources {
        assert!(Arc::ptr_eq(source, &sources[0]), "different snapshots: {sources:?}");
    }
    let later = cache.get(&opts).unwrap();
    assert!(Arc::ptr_eq(&later, &sources[0]), "sequential request fetched again");
    #[cfg(unix)]
    assert_eq!(log.acquisitions().len(), 1);
    let path = later.path().to_path_buf();
    drop(cache);
    assert!(path.exists(), "active consumer lost its snapshot: {path:?}");
    drop((later, sources));
    assert!(!path.exists(), "snapshot was not cleaned: {path:?}");
    let next_install = GitSourceCache::default();
    let next_source = next_install.get(&opts).unwrap();
    assert_ne!(next_source.path(), path);
    #[cfg(unix)]
    assert_eq!(log.acquisitions().len(), 2);
}

#[test]
fn repositories_commits_and_url_spellings_are_isolated() {
    let root = tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "source");
    repo.write_file("value", "first");
    let first = repo.commit("first");
    repo.write_file("value", "second");
    let second = repo.commit("second");
    let other_repo = GitRepoFixture::init(root.path(), "other");
    other_repo.write_file("value", "other");
    let other_commit = other_repo.commit("other");
    let url = repo.file_url();
    let other_url = other_repo.file_url();
    let alternate_url = format!("{url}/");
    let cache = GitSourceCache::default();
    let mut sources = Vec::new();
    for (url, commit, expected) in [
        (&url, &first, "first"),
        (&url, &second, "second"),
        (&other_url, &other_commit, "other"),
        (&alternate_url, &first, "first"),
    ] {
        let source = cache
            .get(&GitSourceOptions { repo: url, commit, git_shallow_hosts: &[], git_bin: None })
            .unwrap();
        assert_eq!(fs::read_to_string(source.path().join("value")).unwrap(), expected);
        for previous in &sources {
            assert!(!Arc::ptr_eq(previous, &source), "distinct keys shared a source");
        }
        sources.push(source);
    }
}

#[test]
fn working_copies_preserve_git_context_without_sharing_mutations() {
    let root = tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "source");
    repo.write_file("value", "original");
    let commit = repo.commit("initial");
    let cache = GitSourceCache::default();
    let source = cache
        .get(&GitSourceOptions {
            repo: &repo.file_url(),
            commit: &commit,
            git_shallow_hosts: &[],
            git_bin: None,
        })
        .unwrap();
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    copy_dir_contents(source.path(), first.path()).unwrap();
    copy_dir_contents(source.path(), second.path()).unwrap();
    fs::write(first.path().join("value"), "changed").unwrap();
    fs::write(first.path().join(".git/HEAD"), "changed").unwrap();
    for original in [source.path(), second.path()] {
        assert_eq!(fs::read_to_string(original.join("value")).unwrap(), "original");
        assert_eq!(fs::read_to_string(original.join(".git/HEAD")).unwrap().trim(), commit);
    }
}

#[cfg(unix)]
#[test]
fn copies_preserve_symlinks_and_executable_files() {
    use std::os::unix::fs::PermissionsExt;
    let source = tempdir().unwrap();
    let target = tempdir().unwrap();
    fs::write(source.path().join("exec"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(source.path().join("exec"), fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("exec", source.path().join("link")).unwrap();
    std::os::unix::fs::symlink("missing", source.path().join("dangling")).unwrap();
    copy_dir_contents(source.path(), target.path()).unwrap();
    assert_eq!(fs::read_link(target.path().join("link")).unwrap(), std::path::Path::new("exec"));
    assert_eq!(
        fs::read_link(target.path().join("dangling")).unwrap(),
        std::path::Path::new("missing"),
    );
    assert_eq!(
        fs::metadata(target.path().join("exec")).unwrap().permissions().mode() & 0o777,
        0o755,
    );
    fs::write(target.path().join("link"), "changed").unwrap();
    assert_eq!(fs::read_to_string(source.path().join("exec")).unwrap(), "#!/bin/sh\n");
}

#[cfg(unix)]
#[test]
fn shallow_and_full_checkouts_of_one_commit_are_isolated() {
    use crate::fetcher::tests::{parse_shim_log, write_git_shim};
    use pnpm_testing_utils::env_guard::EnvGuard;
    let tmp = tempdir().unwrap();
    let shim = write_git_shim(&tmp.path().join("shim"));
    let log_path = tmp.path().join("git-invocations.log");
    let commit = "c9b30e71d704cd30fa71f2edd1ecc7dcc4985493";
    let env = EnvGuard::snapshot(["PACQUET_GIT_SHIM_LOG", "PACQUET_GIT_SHIM_FAKE_COMMIT"]);
    env.set("PACQUET_GIT_SHIM_LOG", &log_path);
    env.set("PACQUET_GIT_SHIM_FAKE_COMMIT", commit);
    let cache = GitSourceCache::default();
    let shallow_hosts = ["test.invalid".to_string()];
    let [shallow, full] = [&shallow_hosts[..], &[]].map(|git_shallow_hosts| {
        cache
            .get(&GitSourceOptions {
                repo: "git://test.invalid/x/y.git",
                commit,
                git_shallow_hosts,
                git_bin: Some(&shim),
            })
            .unwrap()
    });
    assert!(!Arc::ptr_eq(&shallow, &full), "a shallow and a full checkout shared a source");
    let invocations = parse_shim_log(&log_path);
    let operations: Vec<&str> =
        invocations.iter().filter_map(|args| args.first()).map(String::as_str).collect();
    assert!(operations.contains(&"fetch"), "no shallow fetch in {operations:?}");
    assert!(operations.contains(&"clone"), "no full clone in {operations:?}");
}

#[cfg(unix)]
#[test]
fn checkouts_by_different_git_executables_are_isolated() {
    use pnpm_testing_utils::git_repo::GitCommandLog;
    let root = tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "source");
    repo.write_file("package.json", r#"{"name":"source","version":"1.0.0"}"#);
    let commit = repo.commit("initial");
    let url = repo.file_url();
    let cache = GitSourceCache::default();
    let logs = [
        GitCommandLog::new(&root.path().join("first")),
        GitCommandLog::new(&root.path().join("second")),
    ];
    let sources = logs.each_ref().map(|log| {
        cache
            .get(&GitSourceOptions {
                repo: &url,
                commit: &commit,
                git_shallow_hosts: &[],
                git_bin: Some(&log.bin),
            })
            .unwrap()
    });
    assert!(!Arc::ptr_eq(&sources[0], &sources[1]), "different git executables shared a source");
    for log in &logs {
        assert_eq!(log.acquisitions().len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn acquisition_failures_are_shared_and_cleaned_and_a_new_install_retries() {
    use pnpm_testing_utils::git_repo::GitCommandLog;
    let root = tempdir().unwrap();
    let log = GitCommandLog::new(root.path());
    let cache = GitSourceCache::default();
    let missing = root.path().join("missing.git");
    let opts = GitSourceOptions {
        repo: missing.to_str().unwrap(),
        commit: "0123456789012345678901234567890123456789",
        git_shallow_hosts: &[],
        git_bin: Some(&log.bin),
    };
    let barrier = Barrier::new(4);
    let errors = thread::scope(|scope| {
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    cache.get(&opts).unwrap_err()
                })
            })
            .collect();
        tasks.into_iter().map(|task| task.join().unwrap()).collect::<Vec<_>>()
    });
    for error in &errors {
        assert!(Arc::ptr_eq(error, &errors[0]), "waiters did not share the failure");
    }
    let later = cache.get(&opts).unwrap_err();
    assert!(Arc::ptr_eq(&later, &errors[0]), "failure was retried within the install");
    assert_eq!(log.acquisitions().len(), 1);
    assert!(!log.acquisitions()[0].exists(), "partial checkout survived: {:?}", log.acquisitions());
    drop(cache);
    GitSourceCache::default().get(&opts).unwrap_err();
    assert_eq!(log.acquisitions().len(), 2);
    assert!(log.acquisitions().iter().all(|path| !path.exists()), "failed checkouts survived");
}
