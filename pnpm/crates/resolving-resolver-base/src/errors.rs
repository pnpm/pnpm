//! Resolver failures that carry a pnpm error code.
//!
//! [`ResolveError`](crate::ResolveError) type-erases a resolver failure to
//! `Box<dyn Error>`, which drops its [`miette::Diagnostic`] facet — and with
//! it the `ERR_PNPM_*` code that is part of pnpm's public contract. The types
//! here are the exception: a caller that has to keep the code (the dependency
//! tree walker) recovers it by downcasting the box. A resolver therefore has
//! to box them *outermost*, never nested inside another error, or the
//! downcast misses them.

use std::fmt::{self, Write as _};

use chrono::{Local, TimeDelta, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_network::{hide_auth_information, redact_and_sanitize, redact_and_sanitize_multiline};
use pnpm_registry::Package;

/// `ERR_PNPM_NO_MATCHING_VERSION`: the registry served the package's
/// packument, but none of the published versions satisfied the request.
///
/// Distinct from `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`, which means
/// no resolver understood the specifier's shape. A well-formed range that
/// simply matches nothing belongs here (<https://github.com/pnpm/pnpm/issues/13319>).
#[derive(Debug, Display, Error, Diagnostic)]
#[display("No matching version found for {dep} while fetching it from {registry}")]
#[diagnostic(code(ERR_PNPM_NO_MATCHING_VERSION), help("{published_versions}"))]
pub struct NoMatchingVersionError {
    /// The dependency as declared — `<alias>@<bare specifier>`, or the bare
    /// specifier alone when it has no alias.
    #[error(not(source))]
    pub dep: String,
    pub registry: String,
    /// What the registry *does* publish: the latest release, the other
    /// dist-tags, and how to list every version.
    pub published_versions: String,
}

impl NoMatchingVersionError {
    #[must_use]
    pub fn new(dep: String, registry: String, meta: &Package) -> Self {
        Self { dep, registry, published_versions: describe_published_versions(meta) }
    }
}

/// The registry-metadata appendix pnpm prints under a no-matching-version
/// error: the latest release and when it went out, the other dist-tags, and
/// the `pnpm view` invocation that lists every published version.
fn describe_published_versions(meta: &Package) -> String {
    let mut out = String::new();
    if let Some(latest) = meta.dist_tags.get("latest") {
        write!(out, r#"The latest release of {} is "{latest}"."#, meta.name).unwrap();
        if let Some(published_at) = meta.published_at(latest).and_then(stringify_date) {
            write!(out, " Published at {published_at}").unwrap();
        }
        out.push('\n');
    }
    // The tags arrive in a `HashMap`, so they are sorted to keep the message
    // stable across runs rather than left in iteration order.
    let mut other_tags: Vec<_> =
        meta.dist_tags.iter().filter(|(tag, _)| tag.as_str() != "latest").collect();
    other_tags.sort_by_key(|(tag, _)| *tag);
    if !other_tags.is_empty() {
        out.push_str("\nOther releases are:\n");
        for (tag, version) in other_tags {
            write!(out, "  * {tag}: {version}").unwrap();
            if let Some(published_at) = meta.published_at(version).and_then(stringify_date) {
                write!(out, " published at {published_at}").unwrap();
            }
            out.push('\n');
        }
    }
    write!(
        out,
        "\nIf you need the full list of all {} published versions run \"pnpm view {} versions\".",
        meta.versions.len(),
        meta.name,
    )
    .unwrap();
    out
}

/// Render a packument publish timestamp the way pnpm reports it: the date on
/// its own, with the time of day appended while the release is less than a
/// day old. `None` when the registry served a timestamp that doesn't parse.
fn stringify_date(timestamp: &str) -> Option<String> {
    let parsed = crate::parse_packument_timestamp(timestamp)?;
    let local = parsed.with_timezone(&Local);
    if Utc::now() - parsed < TimeDelta::days(1) {
        return Some(local.format("%-m/%-d/%Y %-I:%M:%S %p").to_string());
    }
    Some(local.format("%-m/%-d/%Y").to_string())
}

/// `ERR_PNPM_FETCH_<status>`: the registry answered a request with a non-2xx
/// status. Carries pnpm's hint for the statuses that deny access, so a 404
/// that is really an authorization failure is recognizable as one.
///
/// [`Diagnostic`] is implemented by hand because the code embeds the status.
#[derive(Debug, Display, Error)]
#[display("GET {url}: {status_text} - {status}")]
pub struct RegistryResponseError {
    #[error(not(source))]
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub hint: Option<String>,
}

impl Diagnostic for RegistryResponseError {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new(format!("ERR_PNPM_FETCH_{}", self.status)))
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        self.hint.as_ref().map(|hint| Box::new(hint) as Box<dyn fmt::Display + '_>)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RegistryResponseErrorOptions<'a> {
    /// The requested URL, with any inline credentials already redacted.
    pub url: &'a str,
    pub status: u16,
    /// The status' reason phrase, e.g. `Not Found` for a 404.
    pub status_text: &'a str,
    pub pkg_name: &'a str,
    /// The `Authorization` header value sent with the request. Masked by
    /// [`hide_auth_information`] before it reaches the hint.
    pub auth_header_value: Option<&'a str>,
}

impl RegistryResponseError {
    #[must_use]
    pub fn new(opts: RegistryResponseErrorOptions<'_>) -> Self {
        let RegistryResponseErrorOptions { url, status, status_text, pkg_name, auth_header_value } =
            opts;
        let mut hint = String::new();
        if status == 404 {
            write!(
                hint,
                "{pkg_name} is not in the npm registry, or you have no permission to fetch it.",
            )
            .unwrap();
            if let Some(without_version) = strip_trailing_semver_suffix(pkg_name) {
                write!(hint, " Did you mean {without_version}?").unwrap();
            }
        }
        // Some registries answer an authentication failure with a 404 rather
        // than disclosing that the package exists, so the authorization line
        // is printed for that status too.
        if matches!(status, 401 | 403 | 404) {
            if !hint.is_empty() {
                hint.push_str("\n\n");
            }
            match auth_header_value {
                Some(value) => write!(
                    hint,
                    "An authorization header was used: {}",
                    hide_auth_information(value),
                )
                .unwrap(),
                None => hint.push_str("No authorization header was set for the request."),
            }
        }
        Self {
            url: url.to_string(),
            status,
            status_text: status_text.to_string(),
            hint: (!hint.is_empty()).then_some(hint),
        }
    }
}

/// Detect a package name that accidentally carries a `<version>` suffix
/// (`lodash@4.17.21`, `lodash4.17.21`) and return the part before it, so the
/// 404 hint can suggest the name the user meant. `None` when the name carries
/// no trailing version.
fn strip_trailing_semver_suffix(pkg_name: &str) -> Option<&str> {
    // Common case: `name@version`. A leading `@` (the scope of `@scope/foo`)
    // is not a separator, hence the `index > 0`.
    if let Some(index) = pkg_name.rfind('@')
        && index > 0
        && is_semver(&pkg_name[index + 1..])
    {
        return Some(&pkg_name[..index]);
    }
    // Fallback: a version appended with no separator at all (`foo1.0.0`).
    // Walk backwards over three dot-separated digit blocks.
    let bytes = pkg_name.as_bytes();
    let mut end = consume_trailing_digits(bytes, bytes.len());
    if end == bytes.len() || end == 0 || bytes[end - 1] != b'.' {
        return None;
    }
    end -= 1;
    let before_patch = end;
    end = consume_trailing_digits(bytes, end);
    if end == before_patch || end == 0 || bytes[end - 1] != b'.' {
        return None;
    }
    end -= 1;
    let before_minor = end;
    end = consume_trailing_digits(bytes, end);
    if end == before_minor || end == 0 || !is_semver(&pkg_name[end..]) {
        return None;
    }
    let prefix = pkg_name[..end].strip_suffix('@').unwrap_or(&pkg_name[..end]);
    (!prefix.is_empty()).then_some(prefix)
}

fn consume_trailing_digits(bytes: &[u8], end: usize) -> usize {
    let mut index = end;
    while index > 0 && bytes[index - 1].is_ascii_digit() {
        index -= 1;
    }
    index
}

fn is_semver(candidate: &str) -> bool {
    candidate.parse::<node_semver::Version>().is_ok()
}

/// `ERR_PNPM_GIT_RESOLVE_FAILED`: a git specifier's `git ls-remote` failed —
/// the remote was unreachable, refused the request, or git could not be run —
/// so the committish cannot be pinned to a commit.
///
/// Distinct from the failures that describe the refs a remote *did* serve (an
/// unknown ref, an ambiguous commit-ish): those already name the repository
/// they came from and need no remediation.
///
/// [`Diagnostic`] is implemented by hand because the help is conditional.
#[derive(Debug, Display, Error)]
#[display("Failed to resolve git dependency {specifier:?}: {detail}")]
pub struct GitResolveError {
    #[error(not(source))]
    pub specifier: String,
    /// What git reported, redacted.
    pub detail: String,
    pub hint: Option<String>,
}

impl Diagnostic for GitResolveError {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new("ERR_PNPM_GIT_RESOLVE_FAILED"))
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        self.hint.as_ref().map(|hint| Box::new(hint) as Box<dyn fmt::Display + '_>)
    }
}

impl GitResolveError {
    /// `repo` is the remote the resolution went to; `detail` is git's own
    /// account of the failure, which can echo back a URL carrying
    /// `user:pass@` credentials and is redacted here.
    #[must_use]
    pub fn new(specifier: &str, repo: &str, detail: &str) -> Self {
        Self {
            specifier: redact_and_sanitize(specifier),
            detail: redact_and_sanitize_multiline(detail),
            hint: https_transport_hint(repo),
        }
    }
}

/// Guidance for a specifier that resolved over HTTPS on a machine whose git
/// cannot use that transport, or `None` when the resolution already went over
/// SSH — there, the transport that failed is the one the specifier asked for.
///
/// Substituting the transport is git's job rather than pnpm's: the URL pnpm
/// records has to work for every machine that installs the lockfile, while
/// `insteadOf` rewrites it for this one only.
fn https_transport_hint(repo: &str) -> Option<String> {
    let (scheme, authority) = repo.split_once("://")?;
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let host = authority.split('/').next().unwrap_or(authority);
    let host = host.rsplit_once('@').map_or(host, |(_userinfo, host)| host);
    if host.is_empty() {
        return None;
    }
    // A bracketed IPv6 literal is full of colons, so the port has to be looked
    // for after the closing bracket rather than at the first colon.
    let hostname = match host.split_once(']') {
        Some((address, _port)) if host.starts_with('[') => &host[..=address.len()],
        _ => host.split_once(':').map_or(host, |(hostname, _port)| hostname),
    };
    // The scheme's own port is dropped from the `insteadOf` prefix, matching
    // what git and the TypeScript CLI's `URL` both normalize the remote to.
    let default_port = if scheme == "https" { ":443" } else { ":80" };
    let host = host.strip_suffix(default_port).unwrap_or(host);
    let (host, hostname) = (redact_and_sanitize(host), redact_and_sanitize(hostname));
    Some(format!(
        r#"pnpm resolves this specifier over HTTPS because it does not ask for SSH, and the URL it records has to work on every machine that installs the lockfile.

If git can only reach {hostname} over SSH here, substitute the transport locally, leaving the recorded URL alone:

    git config --global url."git@{hostname}:".insteadOf "{scheme}://{host}/""#,
    ))
}

#[cfg(test)]
mod tests;
