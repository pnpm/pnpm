use super::lock_agent;

#[test]
fn a_second_watcher_cannot_use_a_running_watchers_checkout() {
    let directory = tempfile::tempdir().unwrap();
    let first = lock_agent(directory.path()).unwrap();
    assert!(
        lock_agent(directory.path()).is_err(),
        "a second watcher must not acquire the checkout",
    );
    drop(first);
    assert!(
        lock_agent(directory.path()).is_ok(),
        "the checkout must be released when the watcher stops",
    );
}

#[test]
fn repo_basename_names_the_last_segment_of_a_url_or_a_posix_path() {
    assert_eq!(super::repo_basename("https://github.com/pnpm/pnpm.git"), "pnpm");
    assert_eq!(super::repo_basename("git@github.com:pnpm/pnpm.git"), "pnpm");
    assert_eq!(super::repo_basename("/tmp/demo.git"), "demo");
    assert_eq!(super::repo_basename("/tmp/demo.git/"), "demo");
}

#[cfg(windows)]
#[test]
fn repo_basename_reads_a_backslash_as_a_windows_separator() {
    assert_eq!(super::repo_basename(r"C:\Users\runner\demo.git"), "demo");
    assert_eq!(super::repo_basename(r"C:\Users\runner\demo.git\"), "demo");
}

#[cfg(not(windows))]
#[test]
fn repo_basename_keeps_a_backslash_that_belongs_to_a_posix_file_name() {
    assert_eq!(super::repo_basename(r"/tmp/team\demo.git"), "teamdemo");
}
