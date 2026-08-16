//! Production [`GitProbe`] and [`GitCommandRunner`] implementations.
//!
//! Pulled out from `git_resolver.rs` to keep the public API free of
//! the runner concrete types: callers get either the production
//! pair (real network + real `git` binary) or supply their own
//! ports of the traits in tests.

use std::{future::Future, path::PathBuf, pin::Pin, process::Command, sync::Arc, time::Duration};

use pnpm_network::ThrottledClient;

use crate::{
    git_resolver::{GitProbe, ProbeFuture},
    resolve_ref::{GitCommandRunner, GitRunError},
};

/// Production [`GitProbe`]: issues the HEAD via the install-wide
/// [`ThrottledClient`] (so concurrency-throttling, proxy, TLS, and
/// per-registry config all apply).
pub struct RealGitProbe {
    pub http_client: Arc<ThrottledClient>,
    /// Per-attempt deadline, overriding the client's much larger
    /// `fetchTimeout`. The probe only gates an optimization — failure
    /// falls back to a `type: git` resolution that always works — so
    /// an archive endpoint a firewall blackholes (reachable
    /// `github.com`, blocked `codeload.github.com`) must cost seconds,
    /// not attempts × `fetchTimeout`.
    pub head_timeout: Duration,
}

impl RealGitProbe {
    #[must_use]
    pub fn new(http_client: Arc<ThrottledClient>) -> Self {
        Self { http_client, head_timeout: Duration::from_secs(10) }
    }
}

impl GitProbe for RealGitProbe {
    fn anonymous_head_ok<'a>(&'a self, url: &'a str) -> ProbeFuture<'a> {
        Box::pin(async move {
            let mut delay = Duration::from_millis(500);
            for attempt in 0..3 {
                // Scoped so the throttle permit is released before any
                // backoff sleep.
                let status = {
                    let guard = self.http_client.acquire_for_url(url).await;
                    guard
                        .head(url)
                        .timeout(self.head_timeout)
                        .send()
                        .await
                        .map(|response| response.status())
                        .ok()
                };
                if let Some(status) = status {
                    if status.is_success() {
                        return true;
                    }
                    let transient = status.is_server_error()
                        || matches!(status.as_u16(), 408 | 409 | 420 | 429);
                    if !transient {
                        return false;
                    }
                }
                if attempt < 2 {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
            false
        })
    }
}

/// Production [`GitCommandRunner`].
///
/// Shells out to `git ls-remote -- <repo> [<ref> <ref>^{}]` via
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
        let mut cmd = ls_remote_command(bin, repo, ref_.map(String::as_str));
        let output = cmd.output();
        match output {
            Ok(out) if out.status.success() => {
                return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
            }
            Ok(out) => {
                last_err = Some(String::from_utf8_lossy(&out.stderr).into_owned());
            }
            Err(err) => {
                last_err = Some(spawn_failure(&err));
            }
        }
    }
    Err(GitRunError {
        message: last_err.unwrap_or_else(|| "ls-remote failed with unknown error".to_string()),
    })
}

/// pnpm does not bundle git, so a missing binary is a setup problem rather
/// than a transport one and is worth saying outright.
fn spawn_failure(err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        return "`git` executable not found on PATH. Install git to resolve git-hosted packages."
            .to_string();
    }
    err.to_string()
}

fn ls_remote_command(bin: Option<&PathBuf>, repo: &str, ref_: Option<&str>) -> Command {
    let mut cmd = match bin {
        Some(bin) => Command::new(bin),
        None => Command::new("git"),
    };
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.arg("ls-remote").arg("--").arg(repo);
    if let Some(ref_) = ref_ {
        cmd.arg(ref_).arg(format!("{ref_}^{{}}"));
    }
    cmd
}

#[cfg(test)]
mod tests;
