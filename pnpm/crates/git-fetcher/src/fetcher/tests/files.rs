use super::{
    GitFetcher, ScriptsPrependNodePath, SilentReporter, StoreDir, deny_all_builds, exec_git, fs,
    make_monorepo_bare_repo, tempdir,
};

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_packs_subfolder_when_path_set() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_monorepo_bare_repo(tmp.path());
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let repo_url = format!("file://{}", bare.display());
    let received = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: Some("packages/sub"),
        git_shallow_hosts: &[],
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        store_dir: &store_dir,
        package_id: "sub@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "sub@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let keys: Vec<&str> = received.cas_paths.keys().map(String::as_str).collect();
    assert!(keys.contains(&"package.json"), "sub-dir manifest must be included: {keys:?}");
    assert!(keys.contains(&"index.js"), "sub-dir main must be included: {keys:?}");
    assert!(
        !keys.iter().any(|key| key.contains("other") || key.contains("packages/")),
        "sibling-package files must not appear; keys are relative to the sub-dir: {keys:?}",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_skips_build_when_ignore_scripts() {
    let tmp = tempdir().unwrap();
    // A repo whose `prepare` script would fail if it ran — observing
    // success proves the lifecycle runner never spawned anything.
    let work = tmp.path().join("work");
    let bare = tmp.path().join("repo.git");
    fs::create_dir_all(&work).unwrap();
    exec_git(&["init", "-q", "-b", "main"], Some(&work)).unwrap();
    exec_git(&["config", "user.email", "test@example.invalid"], Some(&work)).unwrap();
    exec_git(&["config", "user.name", "Test"], Some(&work)).unwrap();
    fs::write(
        work.join("package.json"),
        r#"{"name":"x","version":"1.0.0","main":"index.js","scripts":{"prepare":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(work.join("index.js"), "module.exports = 1;\n").unwrap();
    exec_git(&["add", "-A"], Some(&work)).unwrap();
    exec_git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], Some(&work)).unwrap();
    let commit = exec_git(&["rev-parse", "HEAD"], Some(&work)).unwrap().trim().to_string();
    exec_git(&["clone", "--bare", "-q", &work.to_string_lossy(), &bare.to_string_lossy()], None)
        .unwrap();

    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());
    let received = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: deny_all_builds(),
        ignore_scripts: true,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        // The key's `built` dimension reflects what the *dispatcher*
        // would pass for `ignore_scripts: false`. The key would flip to
        // `\tnot-built` when ignore-scripts is honored at the dispatcher
        // layer; pacquet's dispatcher hardcodes `built=true` today (see
        // `install_package_by_snapshot.rs`), so we mirror that here.
        // `received.built` is the unrelated `should_be_built` flag from
        // `prepare_package` (does the manifest declare a build?) — it
        // can be `true` even when scripts were skipped.
        files_index_file: "x@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(
        received.built,
        "should_be_built must still report `true` when the manifest declares prepare scripts, even if ignore_scripts blocked them",
    );
    assert!(received.cas_paths.contains_key("package.json"));
    assert!(received.cas_paths.contains_key("index.js"));
}
