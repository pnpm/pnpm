/// Strip protocol, query string, fragment, basic-auth, and any
/// trailing characters past the path's final `/`, returning the
/// canonical "nerf-darted" form npm uses as `.npmrc` keys.
#[must_use]
pub fn nerf_dart(url: &str) -> String {
    let Some(parsed) = ParsedUrl::parse(url) else { return String::new() };
    parsed.nerf_dart()
}

/// Lightweight URL parsing tuned for the subset of URLs `.npmrc` and
/// registries actually carry: `http`/`https` only, optional `user:pw@`,
/// optional `:port`, optional path. Standard library has no URL type
/// and pulling in the full `url` crate just for this is heavier than
/// needed.
#[derive(Clone, Copy)]
pub(super) struct ParsedUrl<'a> {
    pub(super) scheme: &'a str,
    pub(super) user_info: Option<&'a str>,
    pub(super) host: &'a str,
    pub(super) port: Option<&'a str>,
    pub(super) path: &'a str,
}

impl<'a> ParsedUrl<'a> {
    pub(super) fn parse(url: &'a str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        // Strip query string and fragment. Neither participates in
        // nerf-darting per `removeFragment` / `removeSearch` in npm's
        // own implementation.
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path_tail)) => (authority, path_tail),
            None => (rest, ""),
        };
        let (user_info, host_port) = match authority.rsplit_once('@') {
            Some((user_info, host_port)) => (Some(user_info), host_port),
            None => (None, authority),
        };
        let (host, port) = if host_port.starts_with('[') {
            match host_port.find(']') {
                Some(closing_bracket) => {
                    let host = &host_port[..=closing_bracket];
                    let port = host_port[closing_bracket + 1..].strip_prefix(':');
                    (host, port)
                }
                None => (host_port, None),
            }
        } else {
            match host_port.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (host_port, None),
            }
        };
        Some(ParsedUrl { scheme, user_info, host, port, path })
    }

    pub(super) fn nerf_dart(&self) -> String {
        let mut out = String::with_capacity(2 + self.host.len() + self.path.len());
        out.push_str("//");
        out.push_str(self.host);
        // Drop default ports (`//reg.com:443/` → `//reg.com/`). Without
        // this, a registry configured as `https://reg.com:443/` keys
        // creds at `//reg.com:443/` and a request to
        // `https://reg.com/...` (no port) misses, because the port-strip
        // fallback only fires when the *request* URL carries a port.
        if let Some(port) = self.port
            && !is_default_port(self.scheme, port)
        {
            out.push(':');
            out.push_str(port);
        }
        out.push('/');
        // Drop everything after the last `/` in the path. That final
        // segment is a filename or package selector, not a key.
        let trimmed = match self.path.rfind('/') {
            Some(index) => &self.path[..index],
            None => "",
        };
        if !trimmed.is_empty() {
            out.push_str(trimmed);
            out.push('/');
        }
        out
    }

    pub(super) fn basic_auth_header(&self) -> Option<String> {
        let user_info = self.user_info?;
        let (user, pass) = match user_info.split_once(':') {
            Some((user, pass)) => (user, pass),
            None => (user_info, ""),
        };
        if user.is_empty() && pass.is_empty() {
            return None;
        }
        Some(format!("Basic {}", base64_encode(&format!("{user}:{pass}"))))
    }

    pub(super) fn with_port_stripped(&self) -> ParsedUrl<'a> {
        ParsedUrl { port: None, ..*self }
    }
}

fn is_default_port(scheme: &str, port: &str) -> bool {
    matches!((scheme, port), ("https", "443") | ("http", "80"))
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// Whether credentials may be sent to `url` without crossing a cleartext
/// network link. HTTPS and loopback HTTP endpoints are accepted.
#[must_use]
pub fn is_url_secure_for_credentials(url: &str) -> bool {
    ParsedUrl::parse(url).is_some_and(|parsed| {
        parsed.scheme.eq_ignore_ascii_case("https")
            || (parsed.scheme.eq_ignore_ascii_case("http") && is_loopback_host(parsed.host))
    })
}

/// Local base64 encode so this crate doesn't pull in `base64` just for
/// 4 lines. Standard alphabet, with padding.
#[must_use]
pub fn base64_encode(input: &str) -> String {
    base64_encode_bytes(input.as_bytes())
}

/// [`base64_encode`] for credentials that are not required to be UTF-8,
/// so a re-encoded `.npmrc` credential reproduces its bytes exactly.
#[must_use]
pub fn base64_encode_bytes(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
    }
    let remainder = chunks.remainder();
    match remainder.len() {
        1 => {
            let n = u32::from(remainder[0]) << 16;
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = (u32::from(remainder[0]) << 16) | (u32::from(remainder[1]) << 8);
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}
