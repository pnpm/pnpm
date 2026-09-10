use super::{HostedGitType, Representation};

pub(super) fn protocol_to_representation(protocol: &str) -> Representation {
    match protocol {
        "git+ssh" | "ssh" => Representation::Sshurl,
        "git+https" => Representation::Https,
        "git" => Representation::Git,
        "https" => Representation::Https,
        "http" => Representation::Http,
        _ => Representation::Sshurl,
    }
}

pub(super) fn strip_dot_git(project: &str) -> String {
    project.strip_suffix(".git").unwrap_or(project).to_string()
}

pub(super) struct ParsedUrl {
    pub(super) scheme: String,
    pub(super) username: Option<String>,
    pub(super) password: Option<String>,
    pub(super) host: Option<String>,
    pub(super) pathname: String,
    pub(super) hash: Option<String>,
}

/// Attempt `Url::parse`; if it fails, run upstream's `correctUrl`
/// (handles `scheme://user@host:path` SCP-style URLs) and try again.
/// Mirrors upstream's
/// [`parseGitUrl`](https://github.com/npm/hosted-git-info/blob/v4.1.0/index.js#L221-L237).
pub(super) fn parse_git_url(giturl: &str) -> Option<ParsedUrl> {
    if let Some(parsed) = whatwg_parse(giturl) {
        return Some(parsed);
    }
    whatwg_parse(&correct_url(giturl))
}

/// Convert a `url::Url` (via `reqwest::Url`) into the same fields
/// hosted-git-info reads off Node's `URL`. Falls back to a manual
/// split for non-standard schemes the `url` crate refuses (rare).
pub(super) fn whatwg_parse(giturl: &str) -> Option<ParsedUrl> {
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
pub(super) fn correct_protocol(input: &str) -> String {
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
pub(super) fn correct_url(giturl: &str) -> String {
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

/// The user, project and committish [`super::HostedGit::from_url`] pulls out of a
/// parsed URL, along with the representation that URL shape round-trips to.
pub(super) struct UrlSegments {
    pub(super) user: String,
    pub(super) project: String,
    pub(super) committish: Option<String>,
    pub(super) representation: Representation,
}

/// Shortcut form: pull user/project out of the opaque path. Matches
/// upstream's shortcut branch verbatim.
pub(super) fn shortcut_segments(parsed: &ParsedUrl) -> UrlSegments {
    let mut pathname = parsed.pathname.as_str();
    pathname = pathname.strip_prefix('/').unwrap_or(pathname);
    // Strip auth from the path. Upstream notes "we ignore auth
    // for shortcuts, so just trim it out".
    if let Some(at) = pathname.find('@') {
        pathname = &pathname[at + 1..];
    }
    let (user, project) = match pathname.rfind('/') {
        Some(idx) => (percent_decode(&pathname[..idx]), percent_decode(&pathname[idx + 1..])),
        None => (String::new(), percent_decode(pathname)),
    };
    UrlSegments {
        user,
        project: strip_dot_git(&project),
        committish: parsed
            .hash
            .as_ref()
            .map(|hash| percent_decode(hash.strip_prefix('#').unwrap_or(hash)))
            .filter(|committish| !committish.is_empty()),
        representation: Representation::Shortcut,
    }
}

pub(super) fn host_segments(host_type: HostedGitType, parsed: &ParsedUrl) -> Option<UrlSegments> {
    if !host_type.supports_protocol(&parsed.scheme) {
        return None;
    }
    let segments = extract_for_host(host_type, parsed)?;
    Some(UrlSegments {
        user: percent_decode(&segments.user),
        project: percent_decode(&segments.project),
        committish: segments
            .committish
            .map(|raw| percent_decode(&raw))
            .filter(|decoded| !decoded.is_empty()),
        representation: protocol_to_representation(&parsed.scheme),
    })
}

/// The `user[:password]` credentials to keep, for the protocols that carry
/// them. Shortcut forms have already had their auth trimmed off the path.
pub(super) fn extract_auth(parsed: &ParsedUrl) -> Option<String> {
    let auth_protocols =
        matches!(parsed.scheme.as_str(), "git" | "https" | "git+https" | "http" | "git+http");
    if !auth_protocols {
        return None;
    }
    match (parsed.username.as_deref(), parsed.password.as_deref()) {
        (None, None) => None,
        (user, Some(password)) => Some(format!("{}:{password}", user.unwrap_or(""))),
        (Some(user), None) => Some(user.to_string()),
    }
}

/// `isGitHubShorthand` from upstream. Detects the bare `owner/repo`
/// form that pnpm registers as a github short link.
pub(super) fn is_github_shorthand(arg: &str) -> bool {
    if arg.is_empty() {
        return false;
    }
    let first_hash = arg.find('#');
    let first_slash = arg.find('/');
    let second_slash =
        first_slash.and_then(|first| arg[first + 1..].find('/').map(|rest| first + 1 + rest));

    let has_slash = first_slash.is_some_and(|first| first > 0);
    let does_not_end_with_slash = match first_hash {
        Some(hash) if hash > 0 => arg.as_bytes()[hash - 1] != b'/',
        _ => !arg.ends_with('/'),
    };

    has_slash
        && does_not_end_with_slash
        && !arg.starts_with('.')
        && only_after_hash(arg.find(|ch: char| ch.is_whitespace()), first_hash)
        && only_after_hash(arg.find('@'), first_hash)
        && only_after_hash(arg.find(':'), first_hash)
        && only_after_hash(second_slash, first_hash)
}

/// Whether `pos` is absent or falls after the committish separator: a
/// character that would otherwise disqualify the shorthand is harmless once
/// it is part of the committish.
pub(super) fn only_after_hash(pos: Option<usize>, first_hash: Option<usize>) -> bool {
    pos.is_none_or(|pos| first_hash.is_some_and(|hash| pos > hash))
}

pub(super) struct Segments {
    pub(super) user: String,
    pub(super) project: String,
    pub(super) committish: Option<String>,
}

pub(super) fn extract_for_host(host: HostedGitType, parsed: &ParsedUrl) -> Option<Segments> {
    match host {
        HostedGitType::Github => extract_github(parsed),
        HostedGitType::Bitbucket => extract_bitbucket(parsed),
        HostedGitType::Gitlab => extract_gitlab(parsed),
    }
}

/// Port of `gitHosts.github.extract`.
pub(super) fn extract_github(parsed: &ParsedUrl) -> Option<Segments> {
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
pub(super) fn extract_bitbucket(parsed: &ParsedUrl) -> Option<Segments> {
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
pub(super) fn extract_gitlab(parsed: &ParsedUrl) -> Option<Segments> {
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
pub(super) fn percent_decode(input: &str) -> String {
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
