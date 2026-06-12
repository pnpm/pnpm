//! Ports pnpm's
//! [`parseBareSpecifier.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts).
//!
//! Two-phase API mirrors the upstream split between the sync
//! protocol-prefix dispatch and the async hosted-repo probe:
//!
//! * [`parse_bare_specifier`] runs the synchronous part. Returns
//!   `None` when the input isn't a git-shaped specifier (so the
//!   resolver chain falls through to the next resolver).
//! * [`PartialSpec::finalize`] runs the async part. For hosted
//!   specs it picks between https / ssh based on the
//!   [`GitProbe`] callbacks (HTTP HEAD + `git ls-remote --exit-code`);
//!   for protocol-prefix specs the spec is already complete and the
//!   probe is unused.

use std::{future::Future, pin::Pin};

use crate::hosted_git::{HostedGit, HostedOpts};

/// Fully resolved spec consumed by [`crate::git_resolver::GitResolver`].
///
/// Mirrors upstream's
/// [`HostedPackageSpec`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L8-L21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedPackageSpec {
    /// URL passed to `git ls-remote`. Always carries no committish —
    /// the committish lives in [`Self::git_committish`] /
    /// [`Self::git_range`].
    pub fetch_spec: String,
    /// Original `HostedGit` parse, when the input matched a known
    /// host. Drives [`crate::GitResolver`]'s tarball vs git-resolution
    /// decision.
    pub hosted: Option<HostedGit>,
    /// What the resolver echoes back to the manifest as
    /// `normalizedBareSpecifier`. For hosted inputs this is the
    /// `shortcut()` form; for protocol-prefix inputs it is the
    /// caller's original bare specifier.
    pub normalized_bare_specifier: String,
    pub git_committish: Option<String>,
    pub git_range: Option<String>,
    pub path: Option<String>,
}

/// Output of the sync prefilter [`parse_bare_specifier`].
pub enum PartialSpec {
    /// Hosted input: needs an async probe to decide https/ssh routing.
    Hosted(HostedGit),
    /// Protocol-prefix input: already finalised, no probe needed.
    Direct(HostedPackageSpec),
}

impl PartialSpec {
    /// Drive the async leg. For [`PartialSpec::Direct`] the probe is
    /// ignored.
    pub async fn finalize<Probe: GitProbe + ?Sized>(self, probe: &Probe) -> HostedPackageSpec {
        match self {
            PartialSpec::Direct(spec) => spec,
            PartialSpec::Hosted(hosted) => from_hosted_git(hosted, probe).await,
        }
    }
}

/// Boxed-future return type used by [`GitProbe`]. Same shape as the
/// rest of pacquet's async traits (see `ResolveFuture`).
pub type ProbeFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// Capability seam for the network and git invocations the hosted
/// branch needs.
///
/// Real installs supply an implementation that issues an HTTP HEAD via
/// the install-wide [`pacquet_network::ThrottledClient`] and shells
/// out to `git ls-remote --exit-code`. Tests supply a fake that
/// records calls and yields canned values without touching the
/// network or the system git binary.
pub trait GitProbe: Send + Sync {
    /// `true` when an HTTP HEAD to the given URL returned a 2xx /
    /// 3xx. Used to detect public repos before running `git ls-remote`
    /// (which would otherwise prompt for credentials on a private
    /// repo). Mirrors upstream's
    /// [`isRepoPublic`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L142-L149).
    fn https_head_ok<'a>(&'a self, url: &'a str) -> ProbeFuture<'a>;

    /// `true` when `git ls-remote --exit-code <url> HEAD` exited zero.
    /// Used as a reachability test on both the https and ssh
    /// candidates. Mirrors upstream's
    /// [`accessRepository`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L151-L158).
    fn ls_remote_exit_code<'a>(&'a self, repo: &'a str) -> ProbeFuture<'a>;
}

const GIT_PROTOCOLS: &[&str] =
    &["git", "git+http", "git+https", "git+rsync", "git+ftp", "git+file", "git+ssh", "ssh"];

/// Sync prefilter. Returns `None` when the input isn't a git-shaped
/// specifier — the resolver chain treats this as "no claim" and falls
/// through.
///
/// Mirrors upstream's
/// [`parseBareSpecifier`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L34-L59).
pub fn parse_bare_specifier(bare: &str) -> Option<PartialSpec> {
    if let Some(hosted) = HostedGit::from_url(bare) {
        return Some(PartialSpec::Hosted(hosted));
    }
    let colons_pos = bare.find(':')?;
    let protocol = &bare[..colons_pos];
    let proto_lower = protocol.to_ascii_lowercase();
    let is_git_url = GIT_PROTOCOLS.contains(&proto_lower.as_str())
        || ((proto_lower == "http" || proto_lower == "https") && contains_dot_git_at_end(bare));
    if !is_git_url {
        return None;
    }
    let corrected = correct_url(bare);
    let parsed = reqwest::Url::parse(&corrected).ok()?;
    let hash = parsed.fragment().filter(|f| !f.is_empty()).map(percent_decode_str);
    let params = parse_git_params(hash.as_deref());
    Some(PartialSpec::Direct(HostedPackageSpec {
        fetch_spec: url_to_fetch_spec(&parsed),
        hosted: None,
        normalized_bare_specifier: bare.to_string(),
        git_committish: params.git_committish,
        git_range: params.git_range,
        path: params.path,
    }))
}

/// Check whether the input contains `.git` as a path suffix (`.git#` or
/// `.git` at end-of-string). Mirrors upstream's `/\.git(?:#|$)/` regex.
fn contains_dot_git_at_end(bare: &str) -> bool {
    let mut iter = bare.match_indices(".git");
    iter.any(|(idx, _)| {
        let after = &bare[idx + 4..];
        after.is_empty() || after.starts_with('#')
    })
}

/// Strip the URL's fragment, format it, and drop the `git+` prefix
/// so the result is a plain transport URL. Mirrors upstream's
/// [`urlToFetchSpec`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L61-L68).
fn url_to_fetch_spec(parsed: &reqwest::Url) -> String {
    let mut clone = parsed.clone();
    clone.set_fragment(None);
    let formatted = clone.to_string();
    formatted.strip_prefix("git+").map(str::to_string).unwrap_or(formatted)
}

/// Run upstream's
/// [`correctUrl`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L183-L201)
/// on the input. Strips a leading `git+` and rewrites the SCP-style
/// `ssh://user@host:path` shape into a standard `ssh://user@host/path`
/// so `Url::parse` will accept it.
fn correct_url(input: &str) -> String {
    let mut url = input.strip_prefix("git+").map_or_else(|| input.to_string(), str::to_string);
    if !url.starts_with("ssh://") {
        let mut out = String::with_capacity(url.len() + 4);
        if input.starts_with("git+") {
            out.push_str("git+");
        }
        out.push_str(&url);
        return out;
    }

    // ssh://... case: pull off `#hash` first, split path, look for SCP-style
    // colon in the authority, and convert it to a slash.
    let (head, hash) = match url.find('#') {
        Some(idx) => (url[..idx].to_string(), url[idx..].to_string()),
        None => (url, String::new()),
    };
    url = head;

    let body = &url[6..]; // strip leading "ssh://"
    let (auth, path_parts): (&str, Vec<&str>) = match body.find('/') {
        Some(idx) => (&body[..idx], body[idx + 1..].split('/').collect()),
        None => (body, Vec::new()),
    };
    // After the `@`, the host portion may carry an SCP-style colon
    // that the URL parser cannot consume. Convert the last colon in
    // the host into a `/`, unless it's followed by a numeric port.
    let host = auth.rsplit_once('@').map_or(auth, |(_, host)| host);
    let port_pattern_present = host.rfind(':').is_some_and(|idx| {
        host[idx + 1..].chars().all(|byte| byte.is_ascii_digit()) && !host[idx + 1..].is_empty()
    });
    let host_has_colon = host.contains(':');
    if host_has_colon && !port_pattern_present {
        let auth_parts: Vec<&str> = auth.split(':').collect();
        let protocol = "ssh";
        // `auth_parts[..-1] join ':' + '/' + auth_parts[-1]`
        let new_auth = if auth_parts.len() >= 2 {
            let last = auth_parts[auth_parts.len() - 1];
            let rest = auth_parts[..auth_parts.len() - 1].join(":");
            format!("{rest}/{last}")
        } else {
            auth.to_string()
        };
        let path_tail = if path_parts.is_empty() {
            String::new()
        } else {
            format!("/{}", path_parts.join("/"))
        };
        let prefix = if input.starts_with("git+") { "git+" } else { "" };
        return format!("{prefix}{protocol}://{new_auth}{path_tail}{hash}");
    }

    let prefix = if input.starts_with("git+") { "git+" } else { "" };
    format!("{prefix}{url}{hash}")
}

#[derive(Debug, Default)]
struct GitParsedParams {
    git_committish: Option<String>,
    git_range: Option<String>,
    path: Option<String>,
}

/// Mirrors upstream's
/// [`parseGitParams`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L162-L179).
fn parse_git_params(committish: Option<&str>) -> GitParsedParams {
    let mut out = GitParsedParams::default();
    let Some(committish) = committish else { return out };
    if committish.is_empty() {
        return out;
    }
    for param in committish.split('&') {
        if let Some(range) = param.strip_prefix("semver:") {
            out.git_range = Some(range.to_string());
        } else if let Some(path) = param.strip_prefix("path:") {
            out.path = Some(path.to_string());
        } else {
            out.git_committish = Some(param.to_string());
        }
    }
    out
}

/// Async leg: probe the hosted host for public-vs-private + ssh
/// reachability, pick a `fetchSpec`. Mirrors upstream's
/// [`fromHostedGit`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L70-L132).
async fn from_hosted_git<Probe: GitProbe + ?Sized>(
    hosted: HostedGit,
    probe: &Probe,
) -> HostedPackageSpec {
    let mut fetch_spec: Option<String> = None;

    let git_https_url = hosted.https(HostedGit::no_committish_no_git_plus());
    if let Some(ref https_url) = git_https_url
        && probe.https_head_ok(https_url).await
        && probe.ls_remote_exit_code(https_url).await
    {
        fetch_spec = Some(https_url.clone());
    }

    if fetch_spec.is_none() {
        let ssh_url = hosted.ssh(HostedGit::no_committish());
        if let Some(ref url) = ssh_url
            && probe.ls_remote_exit_code(url).await
        {
            fetch_spec = Some(url.clone());
        }
    }

    if fetch_spec.is_none()
        && let Some(https_url) = hosted.https(HostedGit::no_committish_no_git_plus())
    {
        // Private repo or HEAD probe failed: try `https` (with auth if
        // present) directly, gated on ls-remote reachability.
        let has_auth = hosted.auth.is_some();
        let probe_succeeded = if has_auth || !probe.https_head_ok(&https_url).await {
            probe.ls_remote_exit_code(&https_url).await
        } else {
            false
        };
        if probe_succeeded {
            let params = parse_git_params(hosted.committish.as_deref());
            return HostedPackageSpec {
                fetch_spec: https_url.clone(),
                hosted: Some(strip_committish(hosted)),
                normalized_bare_specifier: format!("git+{https_url}"),
                git_committish: params.git_committish,
                git_range: params.git_range,
                path: params.path,
            };
        }
        // Upstream tries an additional HEAD probe on the bare URL
        // (no `.git` suffix) to confirm the path resolves at all
        // before falling through to ssh. Pacquet mirrors this only
        // when there's no `auth`: with auth, the path is the auth-
        // gated private URL above. Without auth, retest as below.
        if !has_auth {
            let stripped = https_url.strip_suffix(".git").unwrap_or(&https_url);
            if probe.https_head_ok(stripped).await {
                fetch_spec = Some(https_url.clone());
            }
        }
    }

    // Final fallback: `git+ssh` URL form. Matches upstream's
    // `fetchSpec = hosted.sshurl({ noCommittish: true })`.
    let fetch_spec = fetch_spec
        .or_else(|| hosted.sshurl(HostedGit::no_committish()))
        .unwrap_or_else(|| hosted.shortcut(HostedOpts::default()));

    let params = parse_git_params(hosted.committish.as_deref());
    HostedPackageSpec {
        fetch_spec,
        normalized_bare_specifier: hosted.shortcut(HostedOpts::default()),
        hosted: Some(hosted),
        git_committish: params.git_committish,
        git_range: params.git_range,
        path: params.path,
    }
}

fn strip_committish(mut hosted: HostedGit) -> HostedGit {
    hosted.committish = None;
    hosted
}

fn percent_decode_str(input: &str) -> String {
    // See [`crate::hosted_git`]'s `percent_decode` for the same UTF-8
    // reassembly rationale.
    let mut buf: Vec<u8> = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%'
            && idx + 2 < bytes.len()
            && let (Some(hi), Some(lo)) =
                ((bytes[idx + 1] as char).to_digit(16), (bytes[idx + 2] as char).to_digit(16))
        {
            buf.push((hi * 16 + lo) as u8);
            idx += 3;
            continue;
        }
        buf.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8(buf).unwrap_or_else(|_| input.to_string())
}

#[cfg(test)]
mod tests;
