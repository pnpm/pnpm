use super::{
    AllowBuildRef, Diagnostic, GitFetcher, GitFetcherError, Path, ScriptsPrependNodePath,
    SilentReporter, StoreDir, allow_all_builds, deny_all_builds, exec_git, fs, is_safe_repo_arg,
    make_bare_repo, make_bare_repo_with_prepare_script, should_use_shallow, ssh_repo_host, tempdir,
};
#[cfg(unix)]
use super::{
    EnvGuard, failing_fetcher, parse_shim_log, position_of, write_failing_git_shim, write_git_shim,
};

#[test]
fn should_use_shallow_returns_false_for_empty_host_list() {
    assert!(!should_use_shallow("https://github.com/x/y.git", &[]));
}

#[test]
fn should_use_shallow_matches_known_host() {
    let hosts = vec!["github.com".to_string(), "gitlab.com".to_string()];
    assert!(should_use_shallow("https://github.com/x/y.git", &hosts));
    assert!(should_use_shallow("git+ssh://git@github.com/x/y.git", &hosts));
    assert!(!should_use_shallow("https://example.com/x/y.git", &hosts));
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_rejects_option_shaped_commit() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let err = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: "file:///tmp/githost",
        commit: "--upload-pack=touch /tmp/pwned",
        path: None,
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
        package_id: "pkg@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "pkg@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();
    assert!(
        matches!(&err, GitFetcherError::SharedSource(source) if matches!(source.as_ref(), GitFetcherError::InvalidCommit { .. })),
        "expected InvalidCommit, got {err:?}",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_rejects_partial_commit_before_running_git() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let err = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: "file:///tmp/githost",
        commit: "deadbeef",
        path: None,
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
        package_id: "pkg@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "pkg@1.0.0\tbuilt",
        git_bin: Some(Path::new("/definitely/missing/git")),
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    eprintln!("ERROR:\n{err:?}\n");
    assert!(
        matches!(&err, GitFetcherError::SharedSource(source) if matches!(source.as_ref(), GitFetcherError::InvalidCommit { .. })),
        "expected InvalidCommit, got {err:?}",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_imports_package_into_cas() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo(tmp.path());
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
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        store_dir: &store_dir,
        package_id: "pkg@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "pkg@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(!received.built, "package without scripts should not be 'built'");
    assert!(received.cas_paths.contains_key("package.json"));
    assert!(received.cas_paths.contains_key("index.js"));
    let cas_path = &received.cas_paths["package.json"];
    assert!(cas_path.exists(), "CAS entry must exist on disk");
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_rejects_commit_mismatch() {
    let tmp = tempdir().unwrap();
    let (bare, _commit) = make_bare_repo(tmp.path());
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let repo_url = format!("file://{}", bare.display());
    // A SHA that doesn't exist in the repo — `git checkout` will fail
    // before we even reach `rev-parse`, producing a `GitExec` rather
    // than `CheckoutMismatch`. Either path is a hard failure, which is
    // the contract we care about: never silently install a wrong
    // commit.
    let bogus = "0000000000000000000000000000000000000000";
    let err = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: bogus,
        path: None,
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
        package_id: "pkg@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "pkg@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    assert!(
        matches!(&err, GitFetcherError::SharedSource(source) if matches!(source.as_ref(), GitFetcherError::GitExec { .. } | GitFetcherError::CheckoutMismatch { .. })),
        "expected GitExec or CheckoutMismatch, got {err:?}",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_blocks_build_when_not_allowed() {
    let tmp = tempdir().unwrap();
    let work = tmp.path().join("work");
    let bare = tmp.path().join("repo.git");
    fs::create_dir_all(&work).unwrap();
    exec_git(&["init", "-q", "-b", "main"], Some(&work)).unwrap();
    exec_git(&["config", "user.email", "test@example.invalid"], Some(&work)).unwrap();
    exec_git(&["config", "user.name", "Test"], Some(&work)).unwrap();
    fs::write(
        work.join("package.json"),
        r#"{"name":"naughty","version":"2.0.0","main":"index.js","scripts":{"prepare":"tsc"}}"#,
    )
    .unwrap();
    fs::write(work.join("index.js"), "module.exports = 1;\n").unwrap();
    exec_git(&["add", "-A"], Some(&work)).unwrap();
    // `-c commit.gpgsign=false` neutralises a user-global `gpgsign=true`
    // setting that would otherwise demand a real signing key in CI.
    exec_git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], Some(&work)).unwrap();
    let commit = exec_git(&["rev-parse", "HEAD"], Some(&work)).unwrap().trim().to_string();
    exec_git(&["clone", "--bare", "-q", &work.to_string_lossy(), &bare.to_string_lossy()], None)
        .unwrap();

    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());
    let err = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
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
        package_id: "naughty@2.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "naughty@2.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    match err {
        GitFetcherError::Prepare(crate::error::PreparePackageError::NotAllowed {
            name,
            version,
            ..
        }) => {
            assert_eq!(name, "naughty");
            assert_eq!(version, "2.0.0");
        }
        other => panic!("expected Prepare::NotAllowed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_runs_prepare_script_when_allowed() {
    let tmp = tempdir().unwrap();
    // The prepare script writes a marker. Single-quoted inner
    // string so the JSON doesn't need to escape it; node reads the
    // `-e` arg verbatim. Avoid any module/path complications by
    // using node's `fs.writeFileSync` with a relative path that
    // ends up at the prepared `pkg_dir` root.
    let (bare, commit) = make_bare_repo_with_prepare_script(
        tmp.path(),
        r#"node -e "require('fs').writeFileSync('PREPARED.marker', 'ok')""#,
    );
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());

    let received = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: allow_all_builds(),
        ignore_scripts: false,
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
        files_index_file: "x@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(received.built, "manifest with prepare script must report should_be_built=true");
    assert!(
        received.cas_paths.contains_key("PREPARED.marker"),
        "prepare script must have written PREPARED.marker into the prepared tree: keys = {:?}",
        received.cas_paths.keys().collect::<Vec<_>>(),
    );
    assert!(received.cas_paths.contains_key("package.json"));
    assert!(received.cas_paths.contains_key("index.js"));
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_surfaces_prepare_failure() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_prepare_script(
        tmp.path(),
        // node exits 1 → npm install's lifecycle propagates the
        // failure → prepare_package wraps it as
        // ERR_PNPM_PREPARE_PACKAGE.
        r#"node -e "process.exit(1)""#,
    );
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());

    let err = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: allow_all_builds(),
        ignore_scripts: false,
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
        files_index_file: "x@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    // Variant match first so the failure message at the panic site
    // is informative on a `Prepare(InvalidPath {...})` regression
    // (where the diagnostic code is `ERR_PNPM_INVALID_PATH`, not the one we
    // want here).
    match &err {
        GitFetcherError::Prepare(crate::error::PreparePackageError::LifecycleFailed { .. }) => {}
        other => {
            panic!("expected Prepare::LifecycleFailed (ERR_PNPM_PREPARE_PACKAGE), got {other:?}")
        }
    }
    // Then assert the `#[diagnostic(code(...))]` text — a rename of
    // the code on the enum variant (e.g. dropping the
    // `ERR_PNPM_PREPARE_PACKAGE` matcher in favor of a pacquet-only
    // string) would silently regress error-code parity with pnpm
    // without this check.
    use miette::Diagnostic;
    let code = err.code().map(|c| c.to_string()).unwrap_or_default();
    assert_eq!(
        code, "ERR_PNPM_PREPARE_PACKAGE",
        "diagnostic code must match the upstream error contract",
    );
}

/// Mirror of [`fetcher_blocks_build_when_not_allowed`] with
/// `allow_build` returning true. The distinction matters — without
/// this test, a regression that inverted the gate's polarity
/// (block-when-allowed) would still keep the block-test green.
#[tokio::test(flavor = "multi_thread")]
async fn fetcher_runs_prepare_when_allow_build_returns_true() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_prepare_script(
        tmp.path(),
        r#"node -e "require('fs').writeFileSync('BUILD_RAN.marker', 'ok')""#,
    );
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());

    // Targeted allow_build that returns true for *this* package only —
    // catches a regression where the gate ignores the dep path and
    // falls through to default-allow or default-deny.
    let allow_x_only: AllowBuildRef<'_> =
        &|dep_path| dep_path == "x@git+file:///tmp/repo.git#abc123";

    let received = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: allow_x_only,
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        store_dir: &store_dir,
        package_id: "git+file:///tmp/repo.git#abc123",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "git+file:///tmp/repo.git#abc123\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(
        received.built,
        "allow_build returning true must report should_be_built=true (manifest declared prepare)",
    );
    assert!(
        received.cas_paths.contains_key("BUILD_RAN.marker"),
        "allow_build returning true must let the prepare script run: keys = {:?}",
        received.cas_paths.keys().collect::<Vec<_>>(),
    );
}

/// Asserts the shallow-fetch git invocation sequence (`init` →
/// `remote add origin <url>` → `fetch --depth 1 origin <commit>`) by
/// spying on the git binary's argv. Pacquet observes the argv by:
///
/// 1. Writing a tiny shell-script `git` to a temp dir.
/// 2. Passing the shim's path to the fetcher via
///    [`GitFetcher::git_bin`] — process-global `PATH` is *not*
///    touched, so sibling tests in the same binary that call
///    `Command::new("git")` (fixture-setup helpers, ad-hoc test
///    spawns) keep resolving to the real git binary.
/// 3. Letting the fetcher run end-to-end against the shim.
/// 4. Inspecting the shim's append-only log for the expected
///    invocation sequence.
///
/// The shim's two communication channels (log file path, fake
/// commit) ride through env vars. Those vars *do* go through
/// process-global env, but they're only consulted by the shim
/// itself — real git ignores `PACQUET_GIT_SHIM_*`, so a sibling
/// test concurrently spawning `git --version` won't be affected.
/// [`EnvGuard`] serializes the two shim tests against each other
/// so their log-path env vars can't cross-contaminate.
///
/// The shim is Unix-only (it's a `/bin/sh` script). Windows hosts
/// would need a `.cmd` shim and a different process-launch model;
/// out of scope for this test. The [`should_use_shallow_matches_known_host`]
/// unit test already covers the predicate cross-platform, so this
/// test only adds end-to-end argv coverage for the shallow path.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn fetcher_uses_shallow_fetch_for_allowed_hosts() {
    let tmp = tempdir().unwrap();
    let shim_dir = tmp.path().join("shim");
    let log_path = tmp.path().join("git-invocations.log");
    // Any 40-hex value works — the shim echoes it for `rev-parse
    // HEAD` and the fetcher accepts the match.
    let fake_commit = "c9b30e71d704cd30fa71f2edd1ecc7dcc4985493";
    let shim_path = write_git_shim(&shim_dir);

    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    // Use `git://` so `extract_host` returns `Some("test.invalid")`
    // and `should_use_shallow` matches on the configured host. The
    // shim never contacts the URL, so the invalid TLD is harmless.
    let repo_url = "git://test.invalid/x/y.git";
    let shallow_hosts = vec!["test.invalid".to_string()];

    // EnvGuard serializes the two `PACQUET_GIT_SHIM_*` setters so
    // the *other* shim test can't observe our log path. Real git
    // invocations elsewhere in the binary ignore these vars, so
    // they don't need the same lock.
    let env = EnvGuard::snapshot(["PACQUET_GIT_SHIM_LOG", "PACQUET_GIT_SHIM_FAKE_COMMIT"]);
    env.set("PACQUET_GIT_SHIM_LOG", &log_path);
    env.set("PACQUET_GIT_SHIM_FAKE_COMMIT", fake_commit);

    GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: repo_url,
        commit: fake_commit,
        path: None,
        git_shallow_hosts: &shallow_hosts,
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
        package_id: "x@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
        git_bin: Some(&shim_path),
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let invocations = parse_shim_log(&log_path);
    // Sequence — not just presence. A reordered regression
    // (e.g. `fetch` before `remote add`) is what `position_of` +
    // strict-ordering asserts catch.
    let init_at = position_of(&invocations, &["init"])
        .unwrap_or_else(|| panic!("shallow path must call `git init`; got {invocations:?}"));
    let remote_at = position_of(&invocations, &["remote", "add", "origin", "--", repo_url])
        .unwrap_or_else(|| {
            panic!("shallow path must call `git remote add origin -- <url>`; got {invocations:?}")
        });
    let fetch_at = position_of(&invocations, &["fetch", "--depth", "1", "origin", fake_commit])
        .unwrap_or_else(|| {
            panic!(
                "shallow path must call `git fetch --depth 1 origin <commit>`; got {invocations:?}",
            )
        });
    assert!(
        init_at < remote_at && remote_at < fetch_at,
        "shallow sequence must be `init` → `remote add` → `fetch`; got {invocations:?}",
    );
    // `git clone` must NOT appear — that's the non-shallow branch.
    // Without this guard, a future regression that took both paths
    // would still pass the positive assertions above.
    assert!(
        !invocations.iter().any(|args| args.first().map(String::as_str) == Some("clone")),
        "shallow path must NOT call `git clone`; got {invocations:?}",
    );

    drop(env);
}

/// The non-shallow path: same setup as the shallow test, but with
/// the URL's host *outside* `git_shallow_hosts`. The shim must
/// observe a `git clone <url> <dir>` invocation and no `init` /
/// `remote add` / `fetch --depth 1`. Pins both branches of the
/// `should_use_shallow` gate so a future refactor can't silently
/// degrade one to the other.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn fetcher_clones_when_host_not_in_shallow_list() {
    let tmp = tempdir().unwrap();
    let shim_dir = tmp.path().join("shim");
    let log_path = tmp.path().join("git-invocations.log");
    let fake_commit = "0000000000000000000000000000000000000001";
    let shim_path = write_git_shim(&shim_dir);

    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = "git://elsewhere.invalid/x/y.git";
    // Configured host doesn't match the URL's host → non-shallow.
    let shallow_hosts = vec!["test.invalid".to_string()];

    let env = EnvGuard::snapshot(["PACQUET_GIT_SHIM_LOG", "PACQUET_GIT_SHIM_FAKE_COMMIT"]);
    env.set("PACQUET_GIT_SHIM_LOG", &log_path);
    env.set("PACQUET_GIT_SHIM_FAKE_COMMIT", fake_commit);

    GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: repo_url,
        commit: fake_commit,
        path: None,
        git_shallow_hosts: &shallow_hosts,
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
        package_id: "x@1.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
        git_bin: Some(&shim_path),
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let invocations = parse_shim_log(&log_path);
    // `git clone -- <repo_url> <some_path>` — we accept any temp-dir
    // path argument, but pin the leading argv slots. The `--` keeps a
    // `-`-leading repo out of git's option parser.
    assert!(
        invocations.iter().any(|args| {
            args.len() >= 4 && args[0] == "clone" && args[1] == "--" && args[2] == repo_url
        }),
        "non-shallow path must call `git clone -- <url> <dir>`; got {invocations:?}",
    );
    // The shallow argv must be absent — guards the gate's polarity.
    // All three commands the shallow branch issues must be missing,
    // not just the easy-to-spot `init` / `fetch` pair: a regression
    // that took both paths (clone + the shallow sequence) would
    // still pass the positive assertion above.
    for verboten in [
        &["init"][..],
        &["remote", "add", "origin", "--", repo_url],
        &["fetch", "--depth", "1", "origin", fake_commit],
    ] {
        assert!(
            position_of(&invocations, verboten).is_none(),
            "non-shallow path must NOT call `git {}`; got {invocations:?}",
            verboten.join(" "),
        );
    }

    drop(env);
}

#[test]
fn is_safe_repo_arg_rejects_option_shaped_values() {
    assert!(is_safe_repo_arg("https://github.com/x/y.git"));
    assert!(is_safe_repo_arg("file:///tmp/repo"));
    assert!(is_safe_repo_arg("ssh://git@example.com/x/y.git"));

    assert!(!is_safe_repo_arg("--upload-pack=touch /tmp/pwned"));
    assert!(!is_safe_repo_arg("-oProxyCommand=curl evil.example"));
    assert!(!is_safe_repo_arg(""));
    assert!(!is_safe_repo_arg("https://example.com/\0/x"));
}

#[test]
fn ssh_repo_host_recognises_only_ssh_references() {
    assert_eq!(ssh_repo_host("git@github.com:acme/widget.git"), Some("github.com"));
    assert_eq!(ssh_repo_host("ssh://git@github.com/acme/widget.git"), Some("github.com"));
    assert_eq!(ssh_repo_host("git+ssh://git@github.com/acme/widget.git"), Some("github.com"));
    assert_eq!(
        ssh_repo_host("ssh://git@gitlab.example.com:2222/org/repo.git"),
        Some("gitlab.example.com"),
    );
    // Brackets are kept, matching what `URL.hostname` hands the TypeScript CLI.
    assert_eq!(ssh_repo_host("ssh://git@[2001:db8::1]:2222/org/repo.git"), Some("[2001:db8::1]"));
    assert_eq!(ssh_repo_host("ssh://[2001:db8::1]/org/repo.git"), Some("[2001:db8::1]"));

    assert_eq!(ssh_repo_host("https://github.com/acme/widget.git"), None);
    assert_eq!(ssh_repo_host("git://github.com/acme/widget.git"), None);
    assert_eq!(ssh_repo_host("file:///home/zoltan/src/repo"), None);
    assert_eq!(ssh_repo_host(r"C:\src\repo"), None);
}

/// Covers <https://github.com/pnpm/pnpm/issues/13743>.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_clone_over_ssh_names_the_package_and_how_to_re_record_it() {
    let tmp = tempdir().unwrap();
    let shim_path = write_failing_git_shim(&tmp.path().join("shim"));
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let err = failing_fetcher(
        &crate::GitSourceCache::default(),
        "git@github.com:acme/widget.git",
        &store_dir,
        &shim_path,
    )
    .run::<SilentReporter>()
    .await
    .expect_err("the shim fails every clone");

    let GitFetcherError::FetchOverSsh { package, repo, host, stderr } = &err else {
        panic!("expected FetchOverSsh; got {err:?}");
    };
    assert_eq!(package, "@scope/pkg");
    assert_eq!(repo, "git@github.com:acme/widget.git");
    assert_eq!(host, "github.com");
    assert_eq!(stderr, "ssh: connect to host port 22: Connection refused");

    assert_eq!(err.code().expect("a diagnostic code").to_string(), "ERR_PNPM_GIT_FETCH_FAILED");
    let rendered = err.to_string();
    dbg!(&rendered);
    assert!(rendered.contains(r#"Failed to fetch "@scope/pkg""#), "{rendered}");

    let help = err.help().expect("SSH remediation help").to_string();
    dbg!(&help);
    assert!(help.contains("needs an SSH key for github.com"), "{help}");
    assert!(help.contains("pnpm update @scope/pkg"), "{help}");
    assert!(help.contains("do not re-resolve git dependencies"), "{help}");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_clone_over_https_carries_no_ssh_remediation() {
    let tmp = tempdir().unwrap();
    let shim_path = write_failing_git_shim(&tmp.path().join("shim"));
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let err = failing_fetcher(
        &crate::GitSourceCache::default(),
        "https://github.com/acme/widget.git",
        &store_dir,
        &shim_path,
    )
    .run::<SilentReporter>()
    .await
    .expect_err("the shim fails every clone");

    assert!(matches!(err, GitFetcherError::Fetch { .. }), "{err:?}");
    assert_eq!(err.code().expect("a diagnostic code").to_string(), "ERR_PNPM_GIT_FETCH_FAILED");
    assert!(err.help().is_none(), "an HTTPS remote needs no SSH remediation");
}
