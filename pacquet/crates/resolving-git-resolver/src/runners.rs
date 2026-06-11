//! Production [`GitProbe`] and [`GitCommandRunner`] implementations.
//!
//! Pulled out from `git_resolver.rs` to keep the public API free of
//! the runner concrete types: callers get either the production
//! pair (real network + real `git` binary) or supply their own
//! ports of the traits in tests.

use std::{future::Future, path::PathBuf, pin::Pin, process::Stdio, sync::Arc};

use pacquet_network::ThrottledClient;

use crate::{
    parse_bare_specifier::{GitProbe, ProbeFuture},
    resolve_ref::{GitCommandRunner, GitRunError},
};

/// Production [`GitProbe`].
///
/// `https_head_ok` issues an HTTP HEAD via the install-wide
/// [`ThrottledClient`] (so concurrency-throttling, proxy, TLS, and
/// per-registry config all apply). `ls_remote_exit_code` shells out
/// to the system `git` binary.
///
/// `git_bin` overrides the binary path; production callers leave it
/// `None` and the runner resolves `git` through `PATH`.
pub struct RealGitProbe {
    pub http_client: Arc<ThrottledClient>,
    pub git_bin: Option<PathBuf>,
}

impl RealGitProbe {
    #[must_use]
    pub fn new(http_client: Arc<ThrottledClient>) -> Self {
        Self { http_client, git_bin: None }
    }
}

impl GitProbe for RealGitProbe {
    fn https_head_ok<'a>(&'a self, url: &'a str) -> ProbeFuture<'a> {
        Box::pin(async move {
            // Match upstream's `replace(/\.git$/, '')` strip before
            // issuing HEAD — host endpoints serve the human page on
            // the path without `.git`, but reject HEAD on the `.git`
            // alias on some configurations.
            let stripped: &str = url.strip_suffix(".git").unwrap_or(url);
            let guard = self.http_client.acquire().await;
            let response = guard.head(stripped).send().await;
            match response {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            }
        })
    }

    fn ls_remote_exit_code<'a>(&'a self, repo: &'a str) -> ProbeFuture<'a> {
        Box::pin(async move {
            let bin = self.git_bin.as_deref().map(std::path::Path::to_path_buf);
            let repo_owned = repo.to_string();
            tokio::task::spawn_blocking(move || {
                let mut cmd = match bin {
                    Some(b) => std::process::Command::new(b),
                    None => std::process::Command::new("git"),
                };
                cmd.args(["ls-remote", "--exit-code", &repo_owned, "HEAD"]);
                cmd.stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());
                cmd.status().is_ok_and(|s| s.success())
            })
            .await
            .unwrap_or(false)
        })
    }
}

/// Production [`GitCommandRunner`].
///
/// Shells out to `git ls-remote <repo> [<ref> <ref>^{}]` via
/// `tokio::task::spawn_blocking` (the system git CLI is synchronous,
/// and the rest of pacquet keeps the async runtime free of blocking
/// work).
///
/// Mirrors upstream's `graceful-git` "one retry" policy at one extra
/// attempt on transient failure.
pub struct RealGitRunner {
    pub git_bin: Option<PathBuf>,
}

impl RealGitRunner {
    #[must_use]
    pub fn new() -> Self {
        Self { git_bin: None }
    }
}

impl Default for RealGitRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl GitCommandRunner for RealGitRunner {
    fn ls_remote<'a>(
        &'a self,
        repo: &'a str,
        ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        let bin = self.git_bin.as_deref().map(std::path::Path::to_path_buf);
        let repo_owned = repo.to_string();
        let ref_owned = ref_.map(str::to_string);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                run_ls_remote_blocking(bin.as_ref(), &repo_owned, ref_owned.as_ref())
            })
            .await
            .map_err(|err| GitRunError { message: format!("ls-remote task panicked: {err}") })?
        })
    }
}

fn run_ls_remote_blocking(
    bin: Option<&PathBuf>,
    repo: &str,
    ref_: Option<&String>,
) -> Result<String, GitRunError> {
    let attempts = 2; // matches upstream `graceful-git` retries: 1
    let mut last_err: Option<String> = None;
    for _ in 0..attempts {
        let mut cmd = match bin {
            Some(b) => std::process::Command::new(b),
            None => std::process::Command::new("git"),
        };
        cmd.arg("ls-remote").arg(repo);
        if let Some(r) = ref_ {
            cmd.arg(r);
            cmd.arg(format!("{r}^{{}}"));
        }
        let output = cmd.output();
        match output {
            Ok(out) if out.status.success() => {
                return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
            }
            Ok(out) => {
                last_err = Some(String::from_utf8_lossy(&out.stderr).into_owned());
            }
            Err(err) => {
                last_err = Some(err.to_string());
            }
        }
    }
    Err(GitRunError {
        message: last_err.unwrap_or_else(|| "ls-remote failed with unknown error".to_string()),
    })
}
