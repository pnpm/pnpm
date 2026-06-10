use super::{GitFetcher, exec_git_with, extract_host, is_valid_commit_hash, should_use_shallow};
use crate::{error::GitFetcherError, prepare_package::AllowBuildRef};
use pacquet_executor::ScriptsPrependNodePath;
use pacquet_reporter::SilentReporter;
use pacquet_store_dir::StoreDir;
#[cfg(unix)]
use pacquet_testing_utils::env_guard::EnvGuard;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

/// Run `git` (resolved through `PATH`) with `args` and capture stdout —
/// a convenience wrapper around [`exec_git_with`] for fixture setup that
/// does not need to override the binary location.
fn exec_git(args: &[&str], cwd: Option<&Path>) -> Result<String, GitFetcherError> {
    exec_git_with(Path::new("git"), args, cwd)
}

/// Build a bare repo whose manifest declares a `prepare` script. The
/// script is whatever the caller passes — typically a `node -e '…'`
/// one-liner that writes a marker file or exits non-zero. Returns
/// the `(bare_repo_path, commit_sha)` pair the fetcher needs.
fn make_bare_repo_with_prepare_script(tmp: &Path, prepare_script: &str) -> (PathBuf, String) {
    let work = tmp.join("work");
    let bare = tmp.join("repo.git");
    fs::create_dir_all(&work).unwrap();
    exec_git(&["init", "-q", "-b", "main"], Some(&work)).unwrap();
    exec_git(&["config", "user.email", "test@example.invalid"], Some(&work)).unwrap();
    exec_git(&["config", "user.name", "Test"], Some(&work)).unwrap();
    // Manifest with no dependencies so the synthesized `<pm>-install`
    // step has nothing to fetch from a network registry — the test
    // stays self-contained even without verdaccio / a mock registry.
    // The prepare script is plumbed straight in.
    let manifest = format!(
        r#"{{"name":"x","version":"1.0.0","main":"index.js","scripts":{{"prepare":{prepare_script:?}}}}}"#,
    );
    fs::write(work.join("package.json"), manifest).unwrap();
    fs::write(work.join("index.js"), "module.exports = 'src';\n").unwrap();
    exec_git(&["add", "-A"], Some(&work)).unwrap();
    exec_git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], Some(&work)).unwrap();
    let commit = exec_git(&["rev-parse", "HEAD"], Some(&work)).unwrap().trim().to_string();
    exec_git(&["clone", "--bare", "-q", &work.to_string_lossy(), &bare.to_string_lossy()], None)
        .unwrap();
    (bare, commit)
}

fn allow_all_builds<'a>() -> AllowBuildRef<'a> {
    &|_| true
}

/// Create a tiny bare git repo whose single commit ships a
/// `package.json` and `index.js`. Returns `(bare_repo_path,
/// commit_sha)`. The caller passes the bare-path as the fetcher's
/// `repo` (with a `file://` URL prefix so `extract_host` sees it as
/// non-shallow-eligible).
fn make_bare_repo(tmp: &Path) -> (PathBuf, String) {
    let work = tmp.join("work");
    let bare = tmp.join("repo.git");
    fs::create_dir_all(&work).unwrap();

    exec_git(&["init", "-q", "-b", "main"], Some(&work)).unwrap();
    exec_git(&["config", "user.email", "test@example.invalid"], Some(&work)).unwrap();
    exec_git(&["config", "user.name", "Test"], Some(&work)).unwrap();
    fs::write(work.join("package.json"), r#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#)
        .unwrap();
    fs::write(work.join("index.js"), "module.exports = 42;\n").unwrap();
    exec_git(&["add", "-A"], Some(&work)).unwrap();
    // `-c commit.gpgsign=false` neutralises a user-global `gpgsign=true`
    // setting that would otherwise demand a real signing key in CI.
    exec_git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], Some(&work)).unwrap();
    let commit = exec_git(&["rev-parse", "HEAD"], Some(&work)).unwrap().trim().to_string();
    exec_git(&["clone", "--bare", "-q", &work.to_string_lossy(), &bare.to_string_lossy()], None)
        .unwrap();
    (bare, commit)
}

fn deny_all_builds<'a>() -> AllowBuildRef<'a> {
    &|_| false
}

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

#[test]
fn is_valid_commit_hash_accepts_full_sha() {
    assert!(is_valid_commit_hash("c9b30e71d704cd30fa71f2edd1ecc7dcc4985493"));
    assert!(is_valid_commit_hash("C9B30E71D704CD30FA71F2EDD1ECC7DCC4985493"));
}

#[test]
fn is_valid_commit_hash_rejects_short_or_option_shaped_values() {
    assert!(!is_valid_commit_hash("deadbeef"));
    assert!(!is_valid_commit_hash(""));
    assert!(!is_valid_commit_hash("--upload-pack=touch /tmp/pwned"));
    assert!(!is_valid_commit_hash("c9b30e71d704cd30fa71f2edd1ecc7dcc4985493 "));
    // 40 chars but contains a non-hex digit.
    assert!(!is_valid_commit_hash("c9b30e71d704cd30fa71f2edd1ecc7dcc498549z"));
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_rejects_option_shaped_commit() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let err = GitFetcher {
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
        store_dir: &store_dir,
        package_id: "pkg@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "pkg@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();
    assert!(
        matches!(err, GitFetcherError::InvalidCommit { .. }),
        "expected InvalidCommit, got {err:?}",
    );
}

#[test]
fn extract_host_handles_user_authority_and_port() {
    assert_eq!(extract_host("https://github.com/foo/bar"), Some("github.com"));
    assert_eq!(extract_host("git+ssh://git@github.com/foo/bar.git"), Some("github.com"));
    assert_eq!(extract_host("https://host.example:443/foo"), Some("host.example"));
    assert_eq!(extract_host("file:///tmp/x"), None);
    assert_eq!(extract_host("relative/path"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_imports_package_into_cas() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo(tmp.path());
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let repo_url = format!("file://{}", bare.display());
    let received = GitFetcher {
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
        store_dir: &store_dir,
        package_id: "pkg@1.0.0",
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
        store_dir: &store_dir,
        package_id: "pkg@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "pkg@1.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    assert!(
        matches!(err, GitFetcherError::GitExec { .. } | GitFetcherError::CheckoutMismatch { .. }),
        "expected GitExec or CheckoutMismatch, got {err:?}",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_blocks_build_when_not_allowed() {
    let tmp = tempdir().unwrap();
    // A repo whose manifest declares a `prepare` script — exercises
    // the `allow_build` gate without actually spawning the script
    // (the policy is denying-all here).
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
        store_dir: &store_dir,
        package_id: "naughty@2.0.0",
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
        }) => {
            assert_eq!(name, "naughty");
            assert_eq!(version, "2.0.0");
        }
        other => panic!("expected Prepare::NotAllowed, got {other:?}"),
    }
}

/// Variant of `make_bare_repo` for monorepo-style fixtures: commits
/// a `packages/sub/package.json` + `packages/sub/index.js` and a
/// sibling `packages/other/index.js` that must NOT end up in the
/// fetcher's output when `path: Some("packages/sub")` is set.
/// Returns `(bare_repo_path, commit_sha)` like `make_bare_repo`.
fn make_monorepo_bare_repo(tmp: &Path) -> (PathBuf, String) {
    let work = tmp.join("work");
    let bare = tmp.join("repo.git");
    fs::create_dir_all(work.join("packages/sub")).unwrap();
    fs::create_dir_all(work.join("packages/other")).unwrap();

    exec_git(&["init", "-q", "-b", "main"], Some(&work)).unwrap();
    exec_git(&["config", "user.email", "test@example.invalid"], Some(&work)).unwrap();
    exec_git(&["config", "user.name", "Test"], Some(&work)).unwrap();
    fs::write(work.join("package.json"), r#"{"name":"monorepo","version":"0.0.0","private":true}"#)
        .unwrap();
    fs::write(
        work.join("packages/sub/package.json"),
        r#"{"name":"sub","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(work.join("packages/sub/index.js"), "module.exports = 'sub';\n").unwrap();
    fs::write(work.join("packages/other/package.json"), r#"{"name":"other","version":"1.0.0"}"#)
        .unwrap();
    fs::write(work.join("packages/other/index.js"), "module.exports = 'other';\n").unwrap();
    exec_git(&["add", "-A"], Some(&work)).unwrap();
    exec_git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], Some(&work)).unwrap();
    let commit = exec_git(&["rev-parse", "HEAD"], Some(&work)).unwrap().trim().to_string();
    exec_git(&["clone", "--bare", "-q", &work.to_string_lossy(), &bare.to_string_lossy()], None)
        .unwrap();
    (bare, commit)
}

/// Bare repo with no `package.json` at root. Used to confirm the
/// fetcher tolerates packages whose archive lacks a manifest — the
/// install dispatcher rejects such packages downstream, but the
/// fetcher itself must not crash.
fn make_bare_repo_without_manifest(tmp: &Path) -> (PathBuf, String) {
    let work = tmp.join("work");
    let bare = tmp.join("repo.git");
    fs::create_dir_all(&work).unwrap();
    exec_git(&["init", "-q", "-b", "main"], Some(&work)).unwrap();
    exec_git(&["config", "user.email", "test@example.invalid"], Some(&work)).unwrap();
    exec_git(&["config", "user.name", "Test"], Some(&work)).unwrap();
    fs::write(work.join("README.md"), "# bare\n").unwrap();
    fs::write(work.join("index.js"), "module.exports = 1;\n").unwrap();
    exec_git(&["add", "-A"], Some(&work)).unwrap();
    exec_git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], Some(&work)).unwrap();
    let commit = exec_git(&["rev-parse", "HEAD"], Some(&work)).unwrap().trim().to_string();
    exec_git(&["clone", "--bare", "-q", &work.to_string_lossy(), &bare.to_string_lossy()], None)
        .unwrap();
    (bare, commit)
}

/// Ports pnpm's `fetch a package from Git sub folder` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/test/index.ts#L69>.
/// The fetcher must pack only the files under `resolution.path`, not
/// the monorepo root or sibling packages.
#[tokio::test(flavor = "multi_thread")]
async fn fetcher_packs_subfolder_when_path_set() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_monorepo_bare_repo(tmp.path());
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let repo_url = format!("file://{}", bare.display());
    let received = GitFetcher {
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
        store_dir: &store_dir,
        package_id: "sub@1.0.0",
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

/// Ports pnpm's `fetch a package without a package.json` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/test/index.ts#L150>.
/// `prepare_package` returns `should_be_built: false` when no manifest
/// is present, and the fetcher imports whatever files the packlist
/// finds — the install dispatcher rejects manifest-less packages
/// downstream, but the fetcher itself must not crash.
#[tokio::test(flavor = "multi_thread")]
async fn fetcher_handles_repo_without_package_json() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_without_manifest(tmp.path());
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let repo_url = format!("file://{}", bare.display());
    let received = GitFetcher {
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
        store_dir: &store_dir,
        package_id: "anon@0.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "anon@0.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(!received.built, "no manifest → not built");
    assert!(received.cas_paths.contains_key("README.md"));
    assert!(received.cas_paths.contains_key("index.js"));
}

/// Ports pnpm's `do not build the package when scripts are ignored`
/// at <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/test/index.ts#L247>.
/// A repo with `scripts.prepare` set must NOT run the script when
/// `ignore_scripts: true`; the fetcher still reports
/// `should_be_built: true` so the caller knows the package wanted a
/// build (matches upstream's `shouldBeBuilt = true` short-circuit).
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
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        // The key's `built` dimension reflects what the *dispatcher*
        // would pass for `ignore_scripts: false`. Upstream's
        // `pickStoreIndexKey` would flip this to `\tnot-built` when
        // ignore-scripts is honored at the dispatcher layer; pacquet's
        // dispatcher hardcodes `built=true` today (see
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

/// Ports pnpm's `fetch a package from Git that has a prepare script`
/// at <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/test/index.ts#L129>.
/// End-to-end: a manifest with `scripts.prepare` set, `allow_build`
/// returning true, and `ignore_scripts: false` runs the prepare
/// lifecycle. The prepare script writes a marker file; the test
/// confirms the marker lands in `cas_paths`, proving the script
/// actually executed (the file didn't exist in the source tree).
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
        store_dir: &store_dir,
        package_id: "x@1.0.0",
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

/// Ports pnpm's `fail when preparing a git-hosted package` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/test/index.ts#L212>.
/// A prepare script that exits non-zero must surface as
/// `GitFetcherError::Prepare(PreparePackageError::LifecycleFailed)`
/// carrying the `ERR_PNPM_PREPARE_PACKAGE` diagnostic code — the
/// fetcher refuses to add a broken-build snapshot to the CAS.
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
        store_dir: &store_dir,
        package_id: "x@1.0.0",
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
    // (where the diagnostic code is `INVALID_PATH`, not the one we
    // want here).
    match &err {
        GitFetcherError::Prepare(crate::error::PreparePackageError::LifecycleFailed { .. }) => {}
        other => {
            panic!("expected Prepare::LifecycleFailed (ERR_PNPM_PREPARE_PACKAGE), got {other:?}")
        }
    }
    // Then assert the `#[diagnostic(code(...))]` text — a rename of
    // the code on the enum variant (e.g. dropping the upstream
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

/// Ports pnpm's `allow git package with prepare script` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/test/index.ts#L280>.
/// Mirror of the existing `fetcher_blocks_build_when_not_allowed`
/// (line 263) but with `allow_build` returning true: the gate
/// permits the build, the script runs, and the snapshot ships. The
/// distinction matters — without this test, a regression that
/// inverted the gate's polarity (block-when-allowed) would still
/// keep the block-test green.
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
    let allow_x_only: AllowBuildRef<'_> = &|dep_path| dep_path == "x@1.0.0";

    let received = GitFetcher {
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
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
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

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_rejects_untrusted_manifest_identity() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_prepare_script(
        tmp.path(),
        r#"node -e "require('fs').writeFileSync('BUILD_RAN.marker', 'ok')""#,
    );
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());
    let allow_registry_artifacts_only: AllowBuildRef<'_> = &|dep_path| !dep_path.contains("://");

    let err = GitFetcher {
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: allow_registry_artifacts_only,
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@git+file:///tmp/repo.git#abc123",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@git+file:///tmp/repo.git#abc123\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    match err {
        GitFetcherError::Prepare(crate::error::PreparePackageError::NotAllowed {
            name,
            version,
        }) => {
            assert_eq!(name, "x");
            assert_eq!(version, "1.0.0");
        }
        other => panic!("expected NotAllowed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_allows_untrusted_manifest_identity_by_dep_path() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_prepare_script(
        tmp.path(),
        r#"node -e "require('fs').writeFileSync('BUILD_RAN.marker', 'ok')""#,
    );
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());
    let package_id = "x@git+file:///tmp/repo.git#abc123";
    let allow_dep_path: AllowBuildRef<'_> = &|dep_path| dep_path == package_id;

    let received = GitFetcher {
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: allow_dep_path,
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id,
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@git+file:///tmp/repo.git#abc123\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(received.built);
    assert!(received.cas_paths.contains_key("BUILD_RAN.marker"));
}

/// Write a `git` shim shell script to `dir/git` that:
///
/// - Appends every invocation to `$PACQUET_GIT_SHIM_LOG` as one
///   tab-separated line per call.
/// - Fakes `rev-parse HEAD` so it echoes
///   `$PACQUET_GIT_SHIM_FAKE_COMMIT`; the fetcher's commit-match
///   check then passes against a `resolution.commit` set to the
///   same value.
/// - Exits successfully for every other invocation so the fetcher
///   completes its sequence without contacting a remote.
///
/// Both knobs ride through env vars rather than getting baked into
/// the shim's source — keeping `printf '%s' "$VAR"` outside any
/// shell-evaluation context means a `TMPDIR` containing `$` /
/// backticks / `\` can't be re-interpreted by `/bin/sh` when the
/// shim runs. Callers populate the env via the [`EnvGuard`] each
/// test holds.
///
/// Returns the absolute path to the shim binary. The caller passes
/// it as [`GitFetcher::git_bin`] so only *that* fetcher resolves git
/// through the shim — process-global `PATH` is never touched, so
/// sibling tests calling `Command::new("git")` (in fixture setup,
/// or in unrelated git-fetcher tests) keep resolving to the real
/// git binary unaffected.
///
/// The shim handles every `git` invocation the fetcher might make
/// — both the shallow path (`init`, `remote add origin`, `fetch
/// --depth 1 origin`, `checkout`, `rev-parse HEAD`) and the
/// non-shallow `clone`. Each is logged before exit-zero, so both
/// branches of `should_use_shallow` exercise the same shim.
#[cfg(unix)]
fn write_git_shim(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).unwrap();
    let shim_path = dir.join("git");
    // POSIX `sh` (not bash) — every host has `/bin/sh`. The body
    // is a static string: paths/values come from env vars at run
    // time, so no embedded value can be shell-interpreted.
    let body = r#"#!/bin/sh
set -eu
# Tab-separate each argv, terminating with a newline. The trailing
# tab in `printf '%s\t'` becomes a column separator in the log;
# downstream parsing splits on '\t' and drops the empty trailing
# field. Quoting `"$@"` and `"$PACQUET_GIT_SHIM_LOG"` keeps
# whitespace/metachars in arg values from being re-tokenized.
{ printf '%s\t' "$@"; printf '\n'; } >> "$PACQUET_GIT_SHIM_LOG"
# `rev-parse HEAD` is the only invocation whose stdout the fetcher
# actually inspects (to compare against the resolution commit).
if [ "$1" = rev-parse ] && [ "$2" = HEAD ]; then
    printf '%s\n' "$PACQUET_GIT_SHIM_FAKE_COMMIT"
fi
exit 0
"#;
    fs::write(&shim_path, body).unwrap();
    fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755)).unwrap();
    shim_path
}

/// Parse the shim's log into a `Vec<Vec<String>>` of invocations.
/// Each line is `arg<TAB>arg<TAB>...<TAB>` followed by `\n`; the
/// trailing empty field from the terminating tab is dropped so the
/// caller can compare directly against `vec!["init"]` etc.
#[cfg(unix)]
fn parse_shim_log(log_path: &Path) -> Vec<Vec<String>> {
    fs::read_to_string(log_path)
        .unwrap()
        .lines()
        .map(|line| {
            line.split('\t').filter(|part| !part.is_empty()).map(str::to_string).collect::<Vec<_>>()
        })
        .filter(|args| !args.is_empty())
        .collect()
}

/// Return the index of the first invocation matching `argv`, or
/// `None`. Used to assert an *ordered* sequence (`init` before
/// `remote add` before `fetch`) rather than mere presence, which
/// would let a reordered regression slip through.
#[cfg(unix)]
fn position_of(invocations: &[Vec<String>], argv: &[&str]) -> Option<usize> {
    invocations
        .iter()
        .position(|args| args.len() == argv.len() && args.iter().zip(argv).all(|(a, b)| a == b))
}

/// Ports pnpm's `still able to shallow fetch for allowed hosts` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/test/index.ts#L183>.
///
/// Upstream uses `jest.mock('execa')` to spy on the git binary's
/// argv and assert the shallow-fetch sequence (`init` → `remote add
/// origin <url>` → `fetch --depth 1 origin <commit>`). Pacquet
/// achieves the same observation by:
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
/// out of scope for this test. The `should_use_shallow_matches_known_host`
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
        store_dir: &store_dir,
        package_id: "x@1.0.0",
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
    let remote_at = position_of(&invocations, &["remote", "add", "origin", repo_url])
        .unwrap_or_else(|| {
            panic!("shallow path must call `git remote add origin <url>`; got {invocations:?}")
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
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
        git_bin: Some(&shim_path),
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let invocations = parse_shim_log(&log_path);
    // `git clone <repo_url> <some_path>` — we accept any temp-dir
    // path argument, but pin the leading three argv slots.
    assert!(
        invocations
            .iter()
            .any(|args| { args.len() >= 3 && args[0] == "clone" && args[1] == repo_url }),
        "non-shallow path must call `git clone <url> <dir>`; got {invocations:?}",
    );
    // The shallow argv must be absent — guards the gate's polarity.
    // All three commands the shallow branch issues must be missing,
    // not just the easy-to-spot `init` / `fetch` pair: a regression
    // that took both paths (clone + the shallow sequence) would
    // still pass the positive assertion above.
    for verboten in [
        &["init"][..],
        &["remote", "add", "origin", repo_url],
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
