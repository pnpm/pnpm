use super::{
    GitFetcher, GitManifestQuery, exec_git_with, extract_host, is_safe_repo_arg,
    is_valid_commit_hash, read_git_manifest, should_use_shallow, ssh_repo_host,
};
use crate::{
    error::{GitFetcherError, PreparePackageError},
    prepare_package::AllowBuildRef,
};
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::StoreDir;
#[cfg(unix)]
use pnpm_testing_utils::env_guard::EnvGuard;
use serde_json::Value;
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

/// Like [`make_bare_repo`], plus a `packages/foo` sub-package (whose
/// name deliberately differs from the repo root's) and a
/// `packages/no-manifest` directory with no `package.json`.
fn make_bare_repo_with_sub_package(tmp: &Path) -> (PathBuf, String) {
    let work = tmp.join("work-sub");
    let bare = tmp.join("repo-sub.git");
    fs::create_dir_all(work.join("packages/foo")).unwrap();
    fs::create_dir_all(work.join("packages/no-manifest")).unwrap();

    exec_git(&["init", "-q", "-b", "main"], Some(&work)).unwrap();
    exec_git(&["config", "user.email", "test@example.invalid"], Some(&work)).unwrap();
    exec_git(&["config", "user.name", "Test"], Some(&work)).unwrap();
    fs::write(work.join("package.json"), r#"{"name":"the-monorepo","version":"0.0.0"}"#).unwrap();
    fs::write(
        work.join("packages/foo/package.json"),
        r#"{"name":"@scope/foo","version":"2.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(work.join("packages/no-manifest/readme.md"), "no manifest here\n").unwrap();
    exec_git(&["add", "-A"], Some(&work)).unwrap();
    exec_git(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"], Some(&work)).unwrap();
    let commit = exec_git(&["rev-parse", "HEAD"], Some(&work)).unwrap().trim().to_string();
    exec_git(&["clone", "--bare", "-q", &work.to_string_lossy(), &bare.to_string_lossy()], None)
        .unwrap();
    (bare, commit)
}

fn deny_all_builds<'a>() -> AllowBuildRef<'a> {
    &|_| false
}

/// Variant of [`make_bare_repo`] for monorepo-style fixtures: commits
/// a `packages/sub/package.json` + `packages/sub/index.js` and a
/// sibling `packages/other/index.js` that must NOT end up in the
/// fetcher's output when `path: Some("packages/sub")` is set.
/// Returns `(bare_repo_path, commit_sha)` like [`make_bare_repo`].
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
pub(crate) fn write_git_shim(dir: &Path) -> PathBuf {
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
pub(crate) fn parse_shim_log(log_path: &Path) -> Vec<Vec<String>> {
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

/// A `git` shim that fails every invocation, so the transport-failure branch
/// is reachable without a remote.
#[cfg(unix)]
fn write_failing_git_shim(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).unwrap();
    let shim_path = dir.join("git");
    let body = r"#!/bin/sh
printf 'ssh: connect to host port 22: Connection refused\n' >&2
exit 128
";
    fs::write(&shim_path, body).unwrap();
    fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755)).unwrap();
    shim_path
}

/// No shallow hosts, so the fetcher takes the `git clone` branch.
#[cfg(unix)]
fn failing_fetcher<'a>(
    source_cache: &'a crate::GitSourceCache,
    repo: &'a str,
    store_dir: &'a StoreDir,
    git_bin: &'a Path,
) -> GitFetcher<'a> {
    GitFetcher {
        source_cache,
        repo,
        commit: "c9b30e71d704cd30fa71f2edd1ecc7dcc4985493",
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
        store_dir,
        package_id: "git+ssh://git@github.com/acme/widget.git#c9b30e71d704cd30fa71f2edd1ecc7dcc4985493",
        package_name: "@scope/pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "@scope/pkg@1.0.0\tbuilt",
        git_bin: Some(git_bin),
    }
}

mod behavior;

mod integrity;

mod authorization;

mod files;

mod manifests;

mod security;

mod reporting;
