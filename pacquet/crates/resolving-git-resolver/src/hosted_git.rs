//! Pacquet port of the subset of
//! [`hosted-git-info`](https://github.com/npm/hosted-git-info/tree/v4.1.0)
//! that pnpm's git resolver uses. v4.1.0 is the major pinned in pnpm's
//! root `package.json` (catalog entry `hosted-git-info: ^4.1.0`) and is
//! what `node_modules/hosted-git-info/` ships at the time of this port.
//!
//! Coverage:
//!
//! - [`HostedGit::from_url`] recognises the GitHub / GitLab / Bitbucket
//!   shortcut forms (`github:owner/repo#ref`, the bare `owner/repo#ref`
//!   shorthand, `gitlab:…`, `bitbucket:…`), full HTTPS/SSH URLs pointed
//!   at the matching domains, and the `git+ssh:` / `git+https:` / `ssh:` /
//!   `git:` / `http:` / `https:` protocol families. Anything else
//!   (Gitea, self-hosted, generic git+file, ...) returns `None` so the
//!   caller can fall back to the protocol-prefix dispatch in
//!   [`crate::parse_bare_specifier()`].
//! - The output exposes the URL templates the resolver consumes —
//!   `https`, `ssh`, `sshurl`, `tarball`, `shortcut` — each accepting
//!   the `no_committish` / `no_git_plus` flags upstream's `_fill`
//!   threads in.
//!
//! Deliberate deviations from upstream:
//!
//! - The GitLab tarball template emits `/-/archive/<ref>/<project>-<ref>.tar.gz`
//!   directly, matching pnpm's [`gitlabTarballTemplate`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L137-L140)
//!   override (fix [#11533]). Upstream hosted-git-info still emits the
//!   `/api/v4/projects/<user>%2F<project>/repository/archive.tar.gz`
//!   form; pacquet ports the pnpm override, not the raw template.
//! - The `gist` host is not implemented. pnpm's test suite never
//!   exercises it and the install path has no gist-shaped store key.
//! - `browse` / `bugs` / `docs` / `file` / `git` templates are not
//!   implemented — only `https` / `ssh` / `sshurl` / `tarball` /
//!   `shortcut` are used by the resolver.
//!
//! [#11533]: https://github.com/pnpm/pnpm/issues/11533

use std::fmt::{self, Write};

/// Three host families pacquet recognises. Mirrors upstream's
/// `gitHosts` keys at
/// <https://github.com/npm/hosted-git-info/blob/v4.1.0/git-host-info.js>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedGitType {
    Github,
    Gitlab,
    Bitbucket,
}

impl HostedGitType {
    fn domain(self) -> &'static str {
        match self {
            HostedGitType::Github => "github.com",
            HostedGitType::Gitlab => "gitlab.com",
            HostedGitType::Bitbucket => "bitbucket.org",
        }
    }

    fn shortcut_prefix(self) -> &'static str {
        match self {
            HostedGitType::Github => "github",
            HostedGitType::Gitlab => "gitlab",
            HostedGitType::Bitbucket => "bitbucket",
        }
    }

    fn from_shortcut(scheme: &str) -> Option<HostedGitType> {
        match scheme {
            "github" => Some(HostedGitType::Github),
            "gitlab" => Some(HostedGitType::Gitlab),
            "bitbucket" => Some(HostedGitType::Bitbucket),
            _ => None,
        }
    }

    fn from_domain(host: &str) -> Option<HostedGitType> {
        // Strip leading `www.` to match upstream's
        // `parsed.hostname.startsWith('www.') ? parsed.hostname.slice(4) : parsed.hostname`.
        let host = host.strip_prefix("www.").unwrap_or(host);
        match host {
            "github.com" => Some(HostedGitType::Github),
            "gitlab.com" => Some(HostedGitType::Gitlab),
            "bitbucket.org" => Some(HostedGitType::Bitbucket),
            _ => None,
        }
    }

    fn supports_protocol(self, proto: &str) -> bool {
        match self {
            // gitHosts.github.protocols
            HostedGitType::Github => {
                matches!(proto, "git" | "http" | "git+ssh" | "git+https" | "ssh" | "https")
            }
            // gitHosts.gitlab.protocols and gitHosts.bitbucket.protocols
            HostedGitType::Gitlab | HostedGitType::Bitbucket => {
                matches!(proto, "git+ssh" | "git+https" | "ssh" | "https")
            }
        }
    }
}

/// Parsed git host info. Mirrors upstream's `GitHost` instance fields
/// (sans the unused `default` / `opts` slots).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedGit {
    pub host_type: HostedGitType,
    pub user: String,
    pub auth: Option<String>,
    pub project: String,
    pub committish: Option<String>,
    /// The original protocol the URL came in with. Drives the
    /// "default representation" upstream picks for `toString` /
    /// `shortcut` round-trips.
    default_representation: Representation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Representation {
    Shortcut,
    Sshurl,
    Https,
    Git,
    Http,
}

/// Per-call options for the `_fill`-style URL templates.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostedOpts {
    /// Drop the trailing `#<committish>` segment.
    pub no_committish: bool,
    /// Strip the leading `git+` from `https` / `ssh` outputs.
    pub no_git_plus: bool,
}

impl HostedGit {
    /// Convenience: build options that omit the committish.
    #[must_use]
    pub fn no_committish() -> HostedOpts {
        HostedOpts { no_committish: true, no_git_plus: false }
    }

    /// Convenience: drop both `#commit` and the `git+` prefix.
    #[must_use]
    pub fn no_committish_no_git_plus() -> HostedOpts {
        HostedOpts { no_committish: true, no_git_plus: true }
    }
}

impl HostedGit {
    /// Recognise a git URL the way upstream's
    /// [`fromUrl`](https://github.com/npm/hosted-git-info/blob/v4.1.0/index.js#L29-L41)
    /// does.
    ///
    /// Returns `None` when the input names a host pacquet doesn't
    /// recognise (Gitea, self-hosted GitLab, a generic
    /// `git+file://…`, ...), the project is missing, or the URL parses
    /// to an unsupported shape (e.g. a bitbucket `/get/…` archive
    /// URL — upstream's `extract` returns undefined for those and
    /// pacquet mirrors it).
    pub fn from_url(giturl: &str) -> Option<HostedGit> {
        if giturl.is_empty() {
            return None;
        }
        // GitHub shorthand: prepend `github:` and run through the
        // shortcut path. Mirrors upstream's
        // `isGitHubShorthand(giturl) ? 'github:' + giturl : correctProtocol(giturl)`.
        let owned;
        let normalised: &str = if is_github_shorthand(giturl) {
            owned = format!("github:{giturl}");
            &owned
        } else {
            owned = correct_protocol(giturl);
            &owned
        };

        let parsed = parse_git_url(normalised)?;
        // Look up host: shortcut first (so `github://...` wins over the
        // host's full URL parsing), then by domain.
        let shortcut_type = HostedGitType::from_shortcut(&parsed.scheme);
        let domain_type = parsed.host.as_deref().and_then(HostedGitType::from_domain);
        let host_type = shortcut_type.or(domain_type)?;

        let auth_protocols =
            matches!(parsed.scheme.as_str(), "git" | "https" | "git+https" | "http" | "git+http");
        let auth = if auth_protocols && (parsed.username.is_some() || parsed.password.is_some()) {
            let user = parsed.username.as_deref().unwrap_or("");
            if let Some(pw) = parsed.password.as_deref() {
                Some(format!("{user}:{pw}"))
            } else {
                Some(user.to_string())
            }
        } else {
            None
        };

        let (user, project, committish, default_representation) = if shortcut_type.is_some() {
            // Shortcut form: pull user/project out of the opaque
            // path. Matches upstream's shortcut branch verbatim.
            let mut pathname = parsed.pathname.as_str();
            pathname = pathname.strip_prefix('/').unwrap_or(pathname);
            // Strip auth from the path. Upstream notes "we ignore auth
            // for shortcuts, so just trim it out".
            if let Some(at) = pathname.find('@') {
                pathname = &pathname[at + 1..];
            }
            let (user, project) = match pathname.rfind('/') {
                Some(idx) => {
                    let user = percent_decode(&pathname[..idx]);
                    let project = percent_decode(&pathname[idx + 1..]);
                    let user = if user.is_empty() { None } else { Some(user) };
                    (user, project)
                }
                None => (None, percent_decode(pathname)),
            };
            let project = strip_dot_git(&project);
            let committish = parsed
                .hash
                .as_ref()
                .map(|hash| percent_decode(hash.strip_prefix('#').unwrap_or(hash)))
                .filter(|committish| !committish.is_empty());
            let user = user.unwrap_or_default();
            (user, project, committish, Representation::Shortcut)
        } else {
            if !host_type.supports_protocol(&parsed.scheme) {
                return None;
            }
            let segments = extract_for_host(host_type, &parsed)?;
            let user = percent_decode(&segments.user);
            let project = percent_decode(&segments.project);
            let committish = segments
                .committish
                .map(|raw| percent_decode(&raw))
                .filter(|decoded| !decoded.is_empty());
            let representation = protocol_to_representation(&parsed.scheme);
            (user, project, committish, representation)
        };

        if project.is_empty() {
            return None;
        }

        Some(HostedGit { host_type, user, auth, project, committish, default_representation })
    }

    /// Shorthand `<type>:<user>/<project>[#committish]`. Mirrors
    /// upstream's `shortcuttemplate`.
    #[must_use]
    pub fn shortcut(&self, opts: HostedOpts) -> String {
        let mut out =
            format!("{}:{}/{}", self.host_type.shortcut_prefix(), self.user, self.project);
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        out
    }

    /// `git+https://[auth@]<domain>/<user>/<project>.git[#committish]`,
    /// optionally stripped of `git+`. Mirrors upstream's
    /// `httpstemplate` (gitlab and github share the same shape).
    #[must_use]
    pub fn https(&self, opts: HostedOpts) -> Option<String> {
        let auth = self.auth.as_deref().map(|a| format!("{a}@")).unwrap_or_default();
        let mut out = format!(
            "git+https://{auth}{domain}/{user}/{project}.git",
            domain = self.host_type.domain(),
            user = self.user,
            project = self.project,
        );
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        if opts.no_git_plus
            && let Some(stripped) = out.strip_prefix("git+")
        {
            out = stripped.to_string();
        }
        Some(out)
    }

    /// `git@<domain>:<user>/<project>.git[#committish]`. Mirrors
    /// upstream's `sshtemplate`.
    #[must_use]
    pub fn ssh(&self, opts: HostedOpts) -> Option<String> {
        let mut out = format!(
            "git@{domain}:{user}/{project}.git",
            domain = self.host_type.domain(),
            user = self.user,
            project = self.project,
        );
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        Some(out)
    }

    /// `git+ssh://git@<domain>/<user>/<project>.git[#committish]`.
    /// Mirrors upstream's `sshurltemplate`.
    #[must_use]
    pub fn sshurl(&self, opts: HostedOpts) -> Option<String> {
        let mut out = format!(
            "git+ssh://git@{domain}/{user}/{project}.git",
            domain = self.host_type.domain(),
            user = self.user,
            project = self.project,
        );
        if !opts.no_committish
            && let Some(ref c) = self.committish
        {
            out.push('#');
            out.push_str(c);
        }
        if opts.no_git_plus
            && let Some(stripped) = out.strip_prefix("git+")
        {
            out = stripped.to_string();
        }
        Some(out)
    }

    /// Host-specific tarball URL. Mirrors upstream's `tarballtemplate`
    /// per host, with one deviation: GitLab uses the
    /// `/-/archive/<ref>/<project>-<ref>.tar.gz` shape pnpm overrides
    /// the upstream template with at
    /// [parseBareSpecifier.ts:137-140](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/git-resolver/src/parseBareSpecifier.ts#L137-L140)
    /// (fix [#11533](https://github.com/pnpm/pnpm/issues/11533)).
    ///
    /// Returns `None` when no committish is set — every supported host
    /// uses an explicit ref or the literal `HEAD` / `master` placeholder
    /// from upstream's template. Pacquet only ever invokes
    /// `tarball()` after [`crate::resolve_ref::resolve_ref`] has pinned
    /// the commit, so the `None` here is precautionary.
    #[must_use]
    pub fn tarball(&self, opts: HostedOpts) -> Option<String> {
        // Upstream `tarball()` overrides `noCommittish: false`; even
        // when the caller asks to drop the committish elsewhere, the
        // tarball needs a ref. Pacquet mirrors that policy: ignore
        // `opts.no_committish` here.
        let _ = opts;
        let committish = self.committish.as_deref()?;
        let encoded_committish = encode_uri_component(committish);
        Some(match self.host_type {
            HostedGitType::Github => format!(
                "https://codeload.github.com/{user}/{project}/tar.gz/{ref}",
                user = self.user,
                project = self.project,
                r#ref = encoded_committish,
            ),
            HostedGitType::Bitbucket => format!(
                "https://bitbucket.org/{user}/{project}/get/{ref}.tar.gz",
                user = self.user,
                project = self.project,
                r#ref = encoded_committish,
            ),
            HostedGitType::Gitlab => format!(
                "https://gitlab.com/{user}/{project}/-/archive/{ref}/{project}-{ref}.tar.gz",
                user = self.user,
                project = self.project,
                r#ref = encoded_committish,
            ),
        })
    }
}

impl fmt::Display for HostedGit {
    /// Mirrors upstream's `toString`: emit the URL form matching the
    /// default representation; fall back to `sshurl` when the default
    /// isn't a render-able URL (e.g. `shortcut`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opts = HostedOpts::default();
        let rendered = match self.default_representation {
            Representation::Sshurl => self.sshurl(opts),
            Representation::Https | Representation::Http => self.https(opts),
            Representation::Git => self.https(opts),
            Representation::Shortcut => Some(self.shortcut(opts)),
        };
        let rendered = rendered.unwrap_or_else(|| self.shortcut(opts));
        f.write_str(&rendered)
    }
}

fn protocol_to_representation(protocol: &str) -> Representation {
    match protocol {
        "git+ssh" | "ssh" => Representation::Sshurl,
        "git+https" => Representation::Https,
        "git" => Representation::Git,
        "https" => Representation::Https,
        "http" => Representation::Http,
        _ => Representation::Sshurl,
    }
}

fn strip_dot_git(project: &str) -> String {
    project.strip_suffix(".git").unwrap_or(project).to_string()
}

struct ParsedUrl {
    scheme: String,
    username: Option<String>,
    password: Option<String>,
    host: Option<String>,
    pathname: String,
    hash: Option<String>,
}

/// Attempt `Url::parse`; if it fails, run upstream's `correctUrl`
/// (handles `scheme://user@host:path` SCP-style URLs) and try again.
/// Mirrors upstream's
/// [`parseGitUrl`](https://github.com/npm/hosted-git-info/blob/v4.1.0/index.js#L221-L237).
fn parse_git_url(giturl: &str) -> Option<ParsedUrl> {
    if let Some(parsed) = whatwg_parse(giturl) {
        return Some(parsed);
    }
    whatwg_parse(&correct_url(giturl))
}

/// Convert a `url::Url` (via `reqwest::Url`) into the same fields
/// hosted-git-info reads off Node's `URL`. Falls back to a manual
/// split for non-standard schemes the `url` crate refuses (rare).
fn whatwg_parse(giturl: &str) -> Option<ParsedUrl> {
    let parsed = reqwest::Url::parse(giturl).ok()?;
    let scheme = parsed.scheme().to_string();
    let username =
        if parsed.username().is_empty() { None } else { Some(parsed.username().to_string()) };
    let password = parsed.password().map(str::to_string);
    let host = parsed.host_str().map(str::to_string);
    let pathname = if parsed.cannot_be_a_base() {
        // Non-base URLs (e.g. `github:owner/repo`) keep the whole
        // post-scheme tail in `path()`.
        parsed.path().to_string()
    } else {
        parsed.path().to_string()
    };
    let hash = parsed.fragment().map(|f| format!("#{f}"));
    Some(ParsedUrl { scheme, username, password, host, pathname, hash })
}

/// Mirrors upstream's
/// [`correctProtocol`](https://github.com/npm/hosted-git-info/blob/v4.1.0/index.js#L130-L152):
/// for inputs that already use a known scheme, return as-is; for
/// `user@host:path` SCP-style strings, prepend `git+ssh://`; otherwise,
/// insert the missing `//` after the first colon. Pacquet mirrors the
/// `knownProtocols` set (`github:`, `gitlab:`, `bitbucket:`, `http:`,
/// `https:`, `git:`, `git+ssh:`, `git+https:`, `ssh:`).
fn correct_protocol(input: &str) -> String {
    let Some(first_colon) = input.find(':') else {
        return input.to_string();
    };
    let proto = &input[..=first_colon];
    const KNOWN: &[&str] = &[
        "github:",
        "gitlab:",
        "bitbucket:",
        "http:",
        "https:",
        "git:",
        "git+ssh:",
        "git+https:",
        "ssh:",
    ];
    if KNOWN.contains(&proto) {
        return input.to_string();
    }
    if let Some(first_at) = input.find('@') {
        if first_at > first_colon {
            return format!("git+ssh://{input}");
        }
        return input.to_string();
    }
    if let Some(double_slash) = input.find("//")
        && double_slash == first_colon + 1
    {
        return input.to_string();
    }
    format!("{}//{}", &input[..=first_colon], &input[first_colon + 1..])
}

/// SCP-style URL repair. Mirrors upstream's
/// [`correctUrl`](https://github.com/npm/hosted-git-info/blob/v4.1.0/index.js#L183-L216).
fn correct_url(giturl: &str) -> String {
    let first_at = giturl.find('@');
    let last_hash = giturl.rfind('#');
    let _first_colon = giturl.find(':');
    let upper_bound = last_hash.unwrap_or(giturl.len());
    let last_colon = giturl[..upper_bound].rfind(':');

    let mut corrected = giturl.to_string();
    if let (Some(last_colon), Some(first_at)) = (last_colon, first_at)
        && last_colon > first_at
    {
        corrected = format!("{}/{}", &giturl[..last_colon], &giturl[last_colon + 1..]);
    } else if first_at.is_some() && last_colon.is_some() {
        // first_at >= last_colon: leave as-is
    }

    let first_colon = corrected.find(':');
    if first_colon.is_none() && !corrected.contains("//") {
        corrected = format!("git+ssh://{corrected}");
    }
    corrected
}

/// `isGitHubShorthand` from upstream. Detects the bare `owner/repo`
/// form that pnpm registers as a github short link.
fn is_github_shorthand(arg: &str) -> bool {
    // empty input is not a shortcut
    if arg.is_empty() {
        return false;
    }
    let first_hash = arg.find('#');
    let first_slash = arg.find('/');
    let second_slash =
        first_slash.and_then(|first| arg[first + 1..].find('/').map(|rest| first + 1 + rest));
    let first_colon = arg.find(':');
    let first_space = arg.find(|ch: char| ch.is_whitespace());
    let first_at = arg.find('@');

    let space_only_after_hash = first_space.is_none()
        || (first_hash.is_some() && first_space.unwrap() > first_hash.unwrap());
    let at_only_after_hash =
        first_at.is_none() || (first_hash.is_some() && first_at.unwrap() > first_hash.unwrap());
    let colon_only_after_hash = first_colon.is_none()
        || (first_hash.is_some() && first_colon.unwrap() > first_hash.unwrap());
    let second_slash_only_after_hash = second_slash.is_none()
        || (first_hash.is_some() && second_slash.unwrap() > first_hash.unwrap());
    let has_slash = first_slash.is_some_and(|first| first > 0);
    let does_not_end_with_slash = match first_hash {
        Some(hash) if hash > 0 => arg.as_bytes()[hash - 1] != b'/',
        _ => !arg.ends_with('/'),
    };
    let does_not_start_with_dot = !arg.starts_with('.');

    space_only_after_hash
        && has_slash
        && does_not_end_with_slash
        && does_not_start_with_dot
        && at_only_after_hash
        && colon_only_after_hash
        && second_slash_only_after_hash
}

struct Segments {
    user: String,
    project: String,
    committish: Option<String>,
}

fn extract_for_host(host: HostedGitType, parsed: &ParsedUrl) -> Option<Segments> {
    match host {
        HostedGitType::Github => extract_github(parsed),
        HostedGitType::Bitbucket => extract_bitbucket(parsed),
        HostedGitType::Gitlab => extract_gitlab(parsed),
    }
}

/// Port of `gitHosts.github.extract`.
fn extract_github(parsed: &ParsedUrl) -> Option<Segments> {
    let path = parsed.pathname.trim_start_matches('/');
    let mut parts = path.splitn(4, '/');
    let user = parts.next()?.to_string();
    let mut project = parts.next()?.to_string();
    let r#type = parts.next().map(str::to_string);
    let mut committish = parts.next().map(str::to_string);

    if let Some(ref t) = r#type
        && t != "tree"
    {
        return None;
    }

    if r#type.is_none() {
        committish =
            parsed.hash.as_deref().map(|hash| hash.strip_prefix('#').unwrap_or(hash).to_string());
    }

    if project.ends_with(".git") {
        project = project[..project.len() - 4].to_string();
    }

    if user.is_empty() || project.is_empty() {
        return None;
    }

    Some(Segments { user, project, committish })
}

/// Port of `gitHosts.bitbucket.extract`.
fn extract_bitbucket(parsed: &ParsedUrl) -> Option<Segments> {
    let path = parsed.pathname.trim_start_matches('/');
    let mut parts = path.splitn(4, '/');
    let user = parts.next()?.to_string();
    let mut project = parts.next()?.to_string();
    let aux = parts.next().map(str::to_string);

    if aux.as_deref() == Some("get") {
        return None;
    }
    if project.ends_with(".git") {
        project = project[..project.len() - 4].to_string();
    }
    if user.is_empty() || project.is_empty() {
        return None;
    }
    let committish = parsed
        .hash
        .as_deref()
        .map(|hash| hash.strip_prefix('#').unwrap_or(hash).to_string())
        .filter(|committish| !committish.is_empty());
    Some(Segments { user, project, committish })
}

/// Port of `gitHosts.gitlab.extract`.
fn extract_gitlab(parsed: &ParsedUrl) -> Option<Segments> {
    let path = parsed.pathname.trim_start_matches('/').to_string();
    if path.contains("/-/") || path.contains("/archive.tar.gz") {
        return None;
    }
    let mut segments: Vec<&str> = path.split('/').collect();
    let mut project = segments.pop()?.to_string();
    if project.ends_with(".git") {
        project = project[..project.len() - 4].to_string();
    }
    let user = segments.join("/");
    if user.is_empty() || project.is_empty() {
        return None;
    }
    let committish = parsed
        .hash
        .as_deref()
        .map(|hash| hash.strip_prefix('#').unwrap_or(hash).to_string())
        .filter(|committish| !committish.is_empty());
    Some(Segments { user, project, committish })
}

/// Match Node's `decodeURIComponent` for the inputs hosted-git-info
/// sees: percent-decode standard escapes, leave malformed sequences
/// alone (upstream surfaces them via `URIError`, which falls through to
/// returning `None` at the call site). Pacquet keeps the input as-is on
/// malformed input — the affected URLs are caught elsewhere when the
/// downstream parse fails.
fn percent_decode(input: &str) -> String {
    // Decode `%XX` triples to raw bytes first, then reassemble as
    // UTF-8 so multibyte sequences (e.g. a `%E2%80%A6` ellipsis) are
    // reconstructed correctly. Fall back to the original input if the
    // resulting byte stream isn't valid UTF-8 — that matches Node's
    // `decodeURIComponent` throwing a `URIError`, which upstream's
    // `try/catch` in `hosted-git-info`'s `fromUrl` swallows.
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

/// Match Node's `encodeURIComponent`. Percent-encode every byte
/// outside the safe ASCII set Node keeps unencoded:
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
fn encode_uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let safe = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')');
        if safe {
            out.push(byte as char);
        } else {
            write!(&mut out, "%{byte:02X}").expect("write to String never fails");
        }
    }
    out
}

#[cfg(test)]
mod tests;
