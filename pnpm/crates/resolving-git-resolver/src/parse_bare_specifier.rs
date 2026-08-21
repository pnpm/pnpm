//! Recognise and normalise a git-shaped `bareSpecifier`.
//!
//! Parsing is pure: no network, no probing. A hosted spec (`github:`,
//! `gitlab:`, `bitbucket:`, `owner/repo`, or any URL of a known host —
//! HTTPS *and* SSH alike) is treated as an *identity*, not a transport
//! choice, and always finalises to the host's canonical HTTPS URL.
//! Which transport a given machine actually uses to reach the host is
//! that machine's git configuration (credential helpers,
//! `url.<base>.insteadOf` rewrites) — never something recorded in the
//! manifest or lockfile, because it would be wrong on the next machine.
//!
//! * [`parse_bare_specifier`] returns `None` when the input isn't a
//!   git-shaped specifier (so the resolver chain falls through to the
//!   next resolver).
//! * [`PartialSpec::finalize`] completes the hosted branch. URLs of
//!   unknown hosts pass through verbatim — for them the URL *is* the
//!   identity, transport included.

use crate::hosted_git::{HostedGit, HostedOpts};

/// Fully resolved spec consumed by [`crate::git_resolver::GitResolver`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedPackageSpec {
    /// URL passed to `git ls-remote`. Always carries no committish —
    /// the committish lives in [`Self::git_committish`] /
    /// [`Self::git_range`]. For hosted inputs this is the canonical
    /// HTTPS URL regardless of the representation the user wrote.
    pub fetch_spec: String,
    /// Original `HostedGit` parse, when the input matched a known
    /// host and carried no URL-embedded credentials. Makes the repo
    /// eligible for [`crate::GitResolver`]'s host-archive resolution.
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
    /// Input matched a known host: identity parsed, canonical URL
    /// still to be derived.
    Hosted(HostedGit),
    /// URL of an unknown host: already final, kept verbatim.
    Direct(HostedPackageSpec),
}

impl PartialSpec {
    #[must_use]
    pub fn finalize(self) -> HostedPackageSpec {
        match self {
            PartialSpec::Direct(spec) => spec,
            PartialSpec::Hosted(hosted) => from_hosted_git(hosted),
        }
    }
}

const GIT_PROTOCOLS: &[&str] =
    &["git", "git+http", "git+https", "git+rsync", "git+ftp", "git+file", "git+ssh", "ssh"];

/// Sync prefilter. Returns `None` when the input isn't a git-shaped
/// specifier — the resolver chain treats this as "no claim" and falls
/// through.
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
/// `.git` at end-of-string).
fn contains_dot_git_at_end(bare: &str) -> bool {
    let mut iter = bare.match_indices(".git");
    iter.any(|(idx, _)| {
        let after = &bare[idx + 4..];
        after.is_empty() || after.starts_with('#')
    })
}

/// Strip the URL's fragment, format it, and drop the `git+` prefix
/// so the result is a plain transport URL.
fn url_to_fetch_spec(parsed: &reqwest::Url) -> String {
    let mut clone = parsed.clone();
    clone.set_fragment(None);
    let formatted = clone.to_string();
    formatted.strip_prefix("git+").map(str::to_string).unwrap_or(formatted)
}

/// Normalise the input URL: strips a leading `git+` and rewrites the
/// SCP-style `ssh://user@host:path` shape into a standard
/// `ssh://user@host/path` so `Url::parse` will accept it.
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
    // The colons of a bracketed IPv6 literal belong to the address.
    let after_host = if host.starts_with('[') {
        host.find(']').map_or(host, |idx| &host[idx + 1..])
    } else {
        host
    };
    let port_pattern_present = after_host.rfind(':').is_some_and(|idx| {
        after_host[idx + 1..].chars().all(|byte| byte.is_ascii_digit())
            && !after_host[idx + 1..].is_empty()
    });
    let host_has_colon = after_host.contains(':');
    if host_has_colon && !port_pattern_present {
        let protocol = "ssh";
        let new_auth = if let Some(separator) = auth.rfind(':') {
            format!("{}/{}", &auth[..separator], &auth[separator + 1..])
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

/// Parse the `&`-separated committish parameters (`semver:`, `path:`,
/// or a bare committish) carried in a git specifier's fragment.
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

/// Finalise a hosted spec to its canonical HTTPS URL. See the module
/// doc for why no other representation (notably SSH) is ever derived
/// here.
fn from_hosted_git(hosted: HostedGit) -> HostedPackageSpec {
    let params = parse_git_params(hosted.committish.as_deref());
    let https_url = hosted.https(HostedGit::no_committish_no_git_plus());
    // URL-embedded credentials are explicit user content, not
    // transport — and the host's archive endpoint would not carry
    // them, so the spec stays archive-ineligible. `https_url` is the
    // `ls-remote` target, so it carries no committish.
    if hosted.auth.is_some()
        && let Some(https_url) = &https_url
        && let Some(https_specifier) = hosted.https(HostedOpts::default())
    {
        return HostedPackageSpec {
            fetch_spec: https_url.clone(),
            hosted: None,
            normalized_bare_specifier: https_specifier,
            git_committish: params.git_committish,
            git_range: params.git_range,
            path: params.path,
        };
    }
    HostedPackageSpec {
        fetch_spec: https_url.unwrap_or_else(|| hosted.shortcut(HostedOpts::default())),
        normalized_bare_specifier: hosted.shortcut(HostedOpts::default()),
        hosted: Some(hosted),
        git_committish: params.git_committish,
        git_range: params.git_range,
        path: params.path,
    }
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
