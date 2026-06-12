//! Shell out to the system `git` binary, check out the pinned commit,
//! run [`crate::prepare_package()`], delete `.git`, run [`crate::packlist()`],
//! and import the resulting file set into the CAS.
//!
//! Ports pnpm's
//! [`fetching/git-fetcher/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/src/index.ts).
//!
//! Pacquet, like pnpm, does not bundle git — the user must install it.
//! `git` operations are sync (no async git client lives in the workspace),
//! so the work runs under `tokio::task::block_in_place` to keep the
//! current thread off the async runtime's "no-blocking" hot path while
//! still allowing the existing `&StoreDir` / `&AllowBuildPolicy` borrows
//! to flow through without an extra owned-data copy.

use crate::{
    cas_io::{ImportedFiles, import_into_cas},
    error::{GitFetcherError, PreparePackageError},
    packlist::packlist,
    prepare_package::{AllowBuildRef, PreparePackageOptions, PreparedPackage, prepare_package},
};
use pacquet_executor::ScriptsPrependNodePath;
use pacquet_package_manifest::safe_read_package_json_from_dir;
use pacquet_reporter::Reporter;
use pacquet_store_dir::{PackageFilesIndex, StoreDir, StoreIndexWriter};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

/// One-shot fetcher for a single git resolution. Holds borrows for the
/// duration of the call only.
pub struct GitFetcher<'a> {
    pub repo: &'a str,
    pub commit: &'a str,
    /// `path` field from the resolution. `None` packs the repo root.
    pub path: Option<&'a str>,
    /// Hosts that opt into `git init` + `git fetch --depth 1` instead
    /// of a full clone. Mirrors `Config::git_shallow_hosts`.
    pub git_shallow_hosts: &'a [String],
    /// Closure routes through [`crate::prepare_package()`]'s
    /// `allow_build`. The caller (typically the install dispatcher) is
    /// responsible for plumbing whatever policy structure it has into
    /// this closure shape.
    pub allow_build: AllowBuildRef<'a>,
    pub ignore_scripts: bool,
    pub unsafe_perm: bool,
    pub user_agent: Option<&'a str>,
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    pub script_shell: Option<&'a Path>,
    pub node_execpath: Option<&'a Path>,
    pub npm_execpath: Option<&'a Path>,
    pub store_dir: &'a StoreDir,
    /// Used in log lines; matches the `package_id` the rest of the
    /// install dispatcher uses (`<name>@<version>(<peer-suffix>)`).
    pub package_id: &'a str,
    pub requester: &'a str,
    /// Install-scoped store-index writer. When provided, the fetcher
    /// queues a `PackageFilesIndex` row at [`Self::files_index_file`]
    /// after import so a future install's warm prefetch finds the
    /// snapshot in `index.db` and skips the clone/checkout/prepare/
    /// packlist re-run. Passing `None` (e.g., from tests) silently
    /// skips the write — the install is still correct, just slower
    /// on the next run.
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Cache key the row lands at. Mirrors upstream's
    /// `pickStoreIndexKey(resolution, pkgId, { built })` shape — for
    /// git resolutions this is always the `gitHostedStoreIndexKey`
    /// form (`pkg_id\t{built|not-built}`). The dispatcher computes
    /// it once and threads it in.
    pub files_index_file: &'a str,
    /// Override for the `git` binary path. Production callers leave
    /// this `None` and the fetcher resolves `git` through `PATH`
    /// (matches upstream's `execa('git', …)` shape). Tests use it to
    /// inject a shim binary at an absolute path, so the test can
    /// observe the fetcher's argv without mutating process-global
    /// state. `None` keeps the existing `Command::new("git")`
    /// behavior; `Some(path)` runs `Command::new(path)` for every
    /// git invocation inside `run_sync` (`init`, `clone`, `fetch`,
    /// `checkout`, `rev-parse`).
    pub git_bin: Option<&'a Path>,
}

/// Output of [`GitFetcher::run`]. Mirrors the shape of
/// `DownloadTarballToStore::run_without_mem_cache`'s return so the
/// caller can hand it straight into `CreateVirtualDirBySnapshot`.
#[derive(Debug)]
pub struct GitFetchOutput {
    /// Relative-path → CAS-path map. Keys use forward slashes regardless
    /// of host platform (matches upstream's `path/posix` joining).
    pub cas_paths: HashMap<String, PathBuf>,
    /// `shouldBeBuilt` from `prepare_package`. The caller routes this
    /// into the `built` dimension of [`pacquet_store_dir::pick_store_index_key`].
    pub built: bool,
}

impl GitFetcher<'_> {
    /// Run the fetcher. Blocks under
    /// [`tokio::task::block_in_place`] for the git CLI invocations and
    /// the lifecycle-script-running prepare step. Returns the CAS file
    /// map for the prepared sub-directory.
    pub async fn run<Reporter: self::Reporter>(self) -> Result<GitFetchOutput, GitFetcherError> {
        tokio::task::block_in_place(|| self.run_sync::<Reporter>())
    }

    fn run_sync<Reporter: self::Reporter>(self) -> Result<GitFetchOutput, GitFetcherError> {
        if !is_valid_commit_hash(self.commit) {
            return Err(GitFetcherError::InvalidCommit {
                commit: self.commit.to_string(),
                repo: self.repo.to_string(),
            });
        }
        let temp = tempfile::tempdir().map_err(GitFetcherError::Io)?;
        let temp_location = temp.path();

        let git_bin = self.git_bin.unwrap_or_else(|| Path::new("git"));
        if should_use_shallow(self.repo, self.git_shallow_hosts) {
            exec_git_with(git_bin, &["init"], Some(temp_location))?;
            exec_git_with(git_bin, &["remote", "add", "origin", self.repo], Some(temp_location))?;
            exec_git_with(
                git_bin,
                &["fetch", "--depth", "1", "origin", self.commit],
                Some(temp_location),
            )?;
        } else {
            exec_git_with(git_bin, &["clone", self.repo, &temp_location.to_string_lossy()], None)?;
        }

        exec_git_with(git_bin, &["checkout", self.commit], Some(temp_location))?;
        let received = exec_git_with(git_bin, &["rev-parse", "HEAD"], Some(temp_location))?;
        let received_trimmed = received.trim();
        if received_trimmed != self.commit {
            return Err(GitFetcherError::CheckoutMismatch {
                expected: self.commit.to_string(),
                received: received_trimmed.to_string(),
            });
        }

        // `extra_env` is a borrow rather than `Option<&HashMap>` so the
        // shape matches upstream's `runLifecycleHook` options exactly.
        // Bind an empty map to a local so the borrow has the same lifetime
        // as `prepare_opts` itself — relying on `&HashMap::new()`'s
        // temporary-lifetime extension here would work today but is
        // brittle to future expression-reshape edits in this block.
        let empty_env: HashMap<String, String> = HashMap::new();
        let prepare_opts = PreparePackageOptions {
            allow_build: Box::new(|dep_path| (self.allow_build)(dep_path)),
            dep_path: self.package_id,
            ignore_scripts: self.ignore_scripts,
            unsafe_perm: self.unsafe_perm,
            user_agent: self.user_agent,
            scripts_prepend_node_path: self.scripts_prepend_node_path,
            script_shell: self.script_shell,
            node_execpath: self.node_execpath,
            npm_execpath: self.npm_execpath,
            extra_bin_paths: &[],
            extra_env: &empty_env,
        };
        let PreparedPackage { pkg_dir, should_be_built } =
            match prepare_package::<Reporter>(&prepare_opts, temp_location, self.path) {
                Ok(p) => p,
                Err(err) => {
                    return Err(wrap_prepare_error(self.repo, err));
                }
            };
        if self.ignore_scripts && should_be_built {
            tracing::warn!(
                target: "pacquet::git_fetcher",
                repo = %self.repo,
                "the git-hosted package fetched from {} has to be built but the build scripts were ignored",
                self.repo,
            );
        }

        // Match upstream's "delete .git before computing CAS contents"
        // step (`fetching/git-fetcher/src/index.ts:60`) so the resulting
        // files-index is git-history-free. Ignore `NotFound` in case
        // `prepare_package` already wiped it on the rare manifests that
        // do so themselves.
        let dot_git = temp_location.join(".git");
        if let Err(err) = fs::remove_dir_all(&dot_git)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(GitFetcherError::Io(err));
        }

        let manifest = safe_read_package_json_from_dir(&pkg_dir)
            .unwrap_or(None)
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
        let files = packlist(&pkg_dir, &manifest).map_err(GitFetcherError::Packlist)?;

        let ImportedFiles { cas_paths, files_index } =
            import_into_cas(self.store_dir, &pkg_dir, &files)?;

        // Queue a `PackageFilesIndex` row so a future install's warm
        // prefetch finds the snapshot in `index.db` and skips the
        // clone+checkout+prepare+packlist re-run. Mirrors the role
        // of `addFilesFromDir`'s store-index write inside upstream's
        // `fetching/git-fetcher` at index.ts:65-73.
        if let Some(writer) = self.store_index_writer {
            writer.queue(
                self.files_index_file.to_string(),
                PackageFilesIndex {
                    manifest: None,
                    requires_build: Some(should_be_built),
                    algo: "sha512".to_string(),
                    files: files_index,
                    side_effects: None,
                },
            );
        }

        Ok(GitFetchOutput { cas_paths, built: should_be_built })
    }
}

/// Wrap `PreparePackageError` with the same "Failed to prepare
/// git-hosted package fetched from `<repo>`" prefix upstream stamps at
/// [`fetching/git-fetcher/src/index.ts:55-57`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/src/index.ts#L55-L57).
///
/// We do this via the source chain instead of mutating the message
/// (no JS-style `err.message = ...` available), so the wrapped error
/// shows up in `miette`'s rendered chain as "Failed to prepare git-
/// hosted package ... → Failed to prepare package → `ERR_PNPM_PREPARE_PACKAGE`".
fn wrap_prepare_error(_repo: &str, err: PreparePackageError) -> GitFetcherError {
    // For the MVP we preserve `err` as the source; the install log
    // line at the dispatcher level already includes the repo URL via
    // the `package_id` field. A future refactor can add a dedicated
    // `Prepare { repo, source }` variant once we have observed real
    // chains in the install reporter.
    GitFetcherError::Prepare(err)
}

/// True iff `commit` is exactly a 40-character hexadecimal git SHA.
/// Rejects everything else (short SHAs, ref names, option-shaped
/// strings like `--upload-pack=…`) before the value reaches `git`.
fn is_valid_commit_hash(commit: &str) -> bool {
    commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True iff `repo` parses to a host that pacquet should clone via the
/// shallow `init` + `fetch --depth 1` path. Mirrors upstream's
/// [`shouldUseShallow`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/src/index.ts#L81-L91).
fn should_use_shallow(repo: &str, allowed_hosts: &[String]) -> bool {
    if allowed_hosts.is_empty() {
        return false;
    }
    let Some(host) = extract_host(repo) else { return false };
    allowed_hosts.iter().any(|allowed| allowed == host)
}

/// Pluck the host portion out of a git URL. Handles the three forms
/// pnpm's git resolver produces: `https://host/path/...`,
/// `git+ssh://user@host/path/...`, and `git://host/path/...`. Falls
/// through to `None` for `file://` paths and SSH-style
/// `user@host:path/...` (those don't appear in `git_shallow_hosts`
/// defaults and a future PR can flesh them out if needed).
fn extract_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("git://"))
        .or_else(|| url.strip_prefix("git+ssh://"))
        .or_else(|| url.strip_prefix("git+https://"))?;
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    // Strip port, if any.
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() { None } else { Some(host) }
}

/// On Windows, prepend `-c core.longpaths=true` to every git
/// invocation so paths beyond 260 characters don't break checkout.
/// Mirrors upstream's
/// [`prefixGitArgs`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/git-fetcher/src/index.ts#L93-L95).
fn prefix_git_args() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["-c", "core.longpaths=true"]
    }
    #[cfg(not(windows))]
    {
        &[]
    }
}

/// `exec_git` with an explicit binary path. The fetcher uses this so
/// a test-injected shim (via [`GitFetcher::git_bin`]) is resolved at
/// the call site instead of through `PATH`, keeping the shim's
/// observability scope to one fetcher instance rather than the whole
/// process env.
fn exec_git_with(bin: &Path, args: &[&str], cwd: Option<&Path>) -> Result<String, GitFetcherError> {
    let prefix = prefix_git_args();
    let mut cmd = Command::new(bin);
    for arg in prefix {
        cmd.arg(arg);
    }
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            GitFetcherError::GitNotFound
        } else {
            GitFetcherError::Io(err)
        }
    })?;
    if !output.status.success() {
        let operation = static_operation_label(args);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(GitFetcherError::GitExec { operation, stderr, status: output.status });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Return a `'static` label for the git subcommand. Used in error
/// messages. `init` / `clone` / `fetch` / `checkout` / `rev-parse` /
/// `remote` are the only ones the fetcher invokes.
fn static_operation_label(args: &[&str]) -> &'static str {
    let first = args.iter().find(|arg| !arg.starts_with('-')).copied().unwrap_or("git");
    match first {
        "init" => "init",
        "clone" => "clone",
        "remote" => "remote",
        "fetch" => "fetch",
        "checkout" => "checkout",
        "rev-parse" => "rev-parse",
        _ => "git",
    }
}

// `import_into_cas`, `is_file_executable`, and `map_write_cas` live in
// [`crate::cas_io`] so [`crate::GitHostedTarballFetcher`] can reuse
// them for the prepare-and-rewrite pass on git-hosted tarballs.

#[cfg(test)]
mod tests;
