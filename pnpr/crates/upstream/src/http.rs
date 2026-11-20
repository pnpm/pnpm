use super::{
    Arc,
    Duration,
    HeaderMap,
    RedirectGuard,
    ThrottledClient,
    UpstreamConfig,
};

#[derive(Clone)]
pub(super) struct UpstreamHttp {
    pub(super) client: Arc<ThrottledClient>,
    pub(super) fetch_guard: Option<RedirectGuard>,
    pub(super) declared_tarball_client: Option<Arc<ThrottledClient>>,
    pub(super) declared_tarball_guard: Option<RedirectGuard>,
    /// Resolved per-upstream request headers (auth + custom) attached to
    /// every fetch. Empty for an upstream with no `auth:`/`headers:`.
    pub(super) headers: HeaderMap,
    /// The upstream's `timeout`. See [`crate::Deadline`] for what it bounds.
    pub(super) timeout: Duration,
    pub(super) max_packument_size: usize,
}

impl UpstreamHttp {
    pub(super) fn new(config: &UpstreamConfig) -> Self {
        Self {
            client: Arc::new(ThrottledClient::new_for_installs()),
            fetch_guard: None,
            declared_tarball_client: None,
            declared_tarball_guard: None,
            headers: config.headers.clone(),
            timeout: config.requests.timeout,
            max_packument_size: config.requests.max_packument_size,
        }
    }
}

pub(super) const UPSTREAM_ERROR_BODY_LIMIT: usize = 64 * 1024;

pub(super) async fn read_upstream_error_body(response: reqwest::Response) -> String {
    let Ok(body) = pnpm_network::read_limited_body(response, UPSTREAM_ERROR_BODY_LIMIT).await
    else {
        return String::new();
    };
    let raw = String::from_utf8_lossy(&body.bytes);
    let mut text = sanitize_upstream_error_body(&raw);
    if body.truncated {
        if !text.is_empty()
            && !text
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            text.push(' ');
        }
        text.push_str("(response body truncated)");
    }
    text
}

fn sanitize_upstream_error_body(body: &str) -> String {
    if let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(body) {
        sanitize_json_strings(&mut parsed);
        if let Ok(serialized) = serde_json::to_string(&parsed) {
            return pnpr_error::redact_url_credentials(&serialized);
        }
    }
    sanitize_error_text(body)
}

fn sanitize_json_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            *text = sanitize_error_text(text);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                sanitize_json_strings(item);
            }
        }
        serde_json::Value::Object(entries) => {
            let sanitized = std::mem::take(entries)
                .into_iter()
                .map(|(key, mut entry)| {
                    sanitize_json_strings(&mut entry);
                    (sanitize_error_text(&key), entry)
                })
                .collect();
            *entries = sanitized;
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn sanitize_error_text(text: &str) -> String {
    if !text.contains('\\') {
        return pnpr_error::redact_url_credentials(text);
    }
    let unescaped = unescape_json_like(text);
    pnpr_error::redact_url_credentials(&unescaped)
}

fn unescape_json_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            out.push('\\');
            break;
        };
        match escaped {
            '/' | '"' | '\\' => out.push(escaped),
            'u' => decode_ascii_unicode_escape(&mut chars, &mut out),
            other => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

fn decode_ascii_unicode_escape(chars: &mut std::str::Chars<'_>, out: &mut String) {
    let saved = chars.as_str();
    let hex = saved.get(..4);
    if let Some(hex) = hex
        && let Ok(code) = u32::from_str_radix(hex, 16)
        && code <= 0x7f
        && let Some(decoded) = char::from_u32(code)
    {
        for _ in 0..4 {
            let _ = chars.next();
        }
        out.push(decoded);
        return;
    }
    out.push_str(r"\u");
}

/// Whether two URLs share a scheme, host and port, so a credential meant for
/// one may be sent to the other.
pub(super) fn same_origin(base: &str, url: &str) -> bool {
    let (Ok(base), Ok(url)) = (reqwest::Url::parse(base), reqwest::Url::parse(url)) else {
        return false;
    };
    base.scheme() == url.scheme()
        && base
            .host_str()
            .is_some_and(|host| Some(host) == url.host_str())
        && base.port_or_known_default() == url.port_or_known_default()
}
