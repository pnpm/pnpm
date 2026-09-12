/// Mask an `Authorization` header value for display in an error message.
/// The scheme survives so the reader can tell a `Bearer` token from `Basic`
/// credentials, and a token long enough to be recognized by its owner keeps
/// its first four characters; everything else becomes `[hidden]`.
///
/// Control characters are dropped first: the value comes from an untrusted
/// `.npmrc` / environment variable and the masked result is printed to the
/// terminal, so a token carrying raw escapes could otherwise inject terminal
/// output through the characters masking leaves behind.
#[must_use]
pub fn hide_auth_information(auth_header_value: &str) -> String {
    let sanitized: String =
        auth_header_value.chars().filter(|character| !character.is_control()).collect();
    let mut parts = sanitized.split(' ');
    let auth_type = parts.next().unwrap_or_default();
    let Some(token) = parts.next() else {
        return "[hidden]".to_string();
    };
    if token.chars().count() < 20 {
        return format!("{auth_type} [hidden]");
    }
    let prefix: String = token.chars().take(4).collect();
    format!("{auth_type} {prefix}[hidden]")
}

/// Strip `user:pass@` (or `user@`) that appears right after a URL scheme in
/// any message text, e.g. `… https://user:pass@host/pkg …` →
/// `… https://host/pkg …`. A registry configured as `https://user:pass@host/`
/// would otherwise leak its embedded basic-auth into a fetch error or a retry
/// log line.
#[must_use]
pub fn redact_url_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("://") {
        let (before, after) = rest.split_at(pos + "://".len());
        out.push_str(before);
        // Only treat "://" as a URL authority boundary when a scheme character
        // (schemes end in an ASCII alphanumeric) precedes it, so an unrelated
        // "://" in the message isn't mangled.
        let has_scheme = pos > 0 && rest.as_bytes()[pos - 1].is_ascii_alphanumeric();
        rest = strip_leading_userinfo(after).filter(|_| has_scheme).unwrap_or(after);
    }
    out.push_str(rest);
    out
}

/// Make untrusted, URL-bearing text safe to print or log: strip every
/// control character, then redact inline `user:pass@` credentials
/// ([`redact_url_credentials`]). Used for registry URLs and network-error
/// messages alike — both can carry basic-auth or escape sequences from an
/// untrusted `.npmrc` / `--registry` (or a `reqwest` error that echoes the
/// request URL back), which must not leak credentials or inject terminal
/// output via raw escapes / `\r` / `\n`.
///
/// The order is load-bearing: a control character inside the userinfo
/// (`user:pass\r@host`) would split the authority across the redaction
/// scan, and removing it afterwards would rejoin the credentials into the
/// output.
#[must_use]
pub fn redact_and_sanitize(text: &str) -> String {
    let sanitized = sanitize_control_characters(text);
    redact_url_credentials(&sanitized)
}

/// Make a URL safe for user-visible output without exposing credentials,
/// query parameters, fragments, or terminal control characters.
/// Malformed URLs are replaced entirely because their authority cannot be
/// redacted reliably.
#[must_use]
pub fn redact_url_for_display(url: &str) -> String {
    let sanitized = sanitize_control_characters(url);
    let Ok(mut display) = reqwest::Url::parse(&sanitized) else {
        return "[hidden]".to_string();
    };
    if display.set_username("").is_err() || display.set_password(None).is_err() {
        return "[hidden]".to_string();
    }
    display.set_query(None);
    display.set_fragment(None);
    display.to_string()
}

fn sanitize_control_characters(text: &str) -> String {
    text.chars().filter(|character| !character.is_control()).collect()
}

/// [`redact_and_sanitize`] for text whose line breaks are worth keeping, such
/// as a subprocess's multi-line stderr.
///
/// Line breaks are kept only when doing so redacts exactly as much as
/// collapsing the text would: a newline can fall inside a `user:pass@`
/// authority — git echoes such a URL back verbatim, password included — and
/// redacting each line separately would leave the credentials split but
/// readable. Whenever the two disagree, the collapsed form wins.
#[must_use]
pub fn redact_and_sanitize_multiline(text: &str) -> String {
    let collapsed = redact_and_sanitize(text);
    let per_line = text.split('\n').map(redact_and_sanitize).collect::<Vec<_>>().join("\n");
    if per_line.replace('\n', "") == collapsed { per_line } else { collapsed }
}

/// If the authority leading `text` contains `userinfo@`, return the slice after
/// the **last** `@` within it; otherwise `None`. The authority ends at the first
/// `/`, `?`, `#`, or whitespace. Stripping to the last `@` keeps a raw `@` inside
/// the password (`user:p@ss@host`) from leaking its tail.
fn strip_leading_userinfo(authority: &str) -> Option<&str> {
    let mut last_at = None;
    for (idx, ch) in authority.char_indices() {
        match ch {
            '@' => last_at = Some(idx + ch.len_utf8()),
            '/' | '?' | '#' => break,
            c if c.is_whitespace() => break,
            _ => {}
        }
    }
    last_at.map(|end| &authority[end..])
}
