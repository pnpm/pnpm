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
fn repo_basename_extracts_clean_name_from_windows_and_unix_paths() {
    assert_eq!(super::repo_basename("https://github.com/pnpm/pnpm.git"), "pnpm");
    assert_eq!(super::repo_basename("/tmp/demo.git"), "demo");
    assert_eq!(super::repo_basename(r"C:\Users\runner\demo.git"), "demo");
    assert_eq!(super::repo_basename(r"C:\Users\runner\demo.git\"), "demo");
}
