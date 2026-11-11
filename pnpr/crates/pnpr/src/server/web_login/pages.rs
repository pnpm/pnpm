//! The HTML pages of the CLI web login: the code form and, for a pnpr with
//! several OIDC login providers, the provider choice.

use std::{fmt::Write as _, sync::LazyLock};

use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use sha2::{Digest as _, Sha256};

use super::super::oidc::protect;

pub(super) const UNUSABLE_CODE: &str = "That code does not match a pending sign-in. Check the code in your terminal and try again. \
     If it expired or was already used, run the login command again.";
pub(super) const PROVIDER_UNAVAILABLE: &str =
    "The sign-in provider is not reachable. Try again in a moment.";

const STYLE: &str = r#":root{color-scheme:light dark;--bg:#f4f5f7;--card:#fff;--text:#1d2127;--muted:#5b6470;--border:#d9dde3;--accent:#f69220;--on-accent:#1d2127;--error:#b42318;--error-bg:#fdecec}
@media (prefers-color-scheme:dark){:root{--bg:#111418;--card:#1b1f24;--text:#e8eaed;--muted:#9aa3ad;--border:#2f353d;--error:#ff8a80;--error-bg:#3a1d1d}}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:16px;background:var(--bg);color:var(--text);font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{width:100%;max-width:400px;background:var(--card);border:1px solid var(--border);border-radius:12px;padding:32px}
h1{margin:0 0 8px;font-size:1.375rem;line-height:1.3}
p{margin:0 0 20px;color:var(--muted)}
.error{color:var(--error);background:var(--error-bg);border-radius:8px;padding:10px 12px}
label{display:block;font-weight:600;margin-bottom:6px}
input{width:100%;padding:12px;font:600 1.5rem/1.2 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;letter-spacing:.15em;text-align:center;text-transform:uppercase;color:var(--text);background:var(--bg);border:1px solid var(--border);border-radius:8px}
input:focus{outline:2px solid var(--accent);outline-offset:1px}
button{display:block;width:100%;margin-top:16px;padding:12px;font:inherit;font-weight:600;color:var(--on-accent);background:var(--accent);border:0;border-radius:8px;cursor:pointer}
button+button{margin-top:8px}
button:hover{filter:brightness(1.06)}
button:focus-visible{outline:2px solid var(--text);outline-offset:2px}"#;

/// The pages' Content Security Policy: [`protect`]'s, plus [`STYLE`] by hash.
static CONTENT_SECURITY_POLICY: LazyLock<HeaderValue> = LazyLock::new(|| {
    let hash = BASE64_STANDARD.encode(Sha256::digest(STYLE.as_bytes()));
    HeaderValue::from_str(&format!(
        "default-src 'none'; style-src 'sha256-{hash}'; frame-ancestors 'none'",
    ))
    .expect("the policy is ASCII")
});

fn html_page(status: StatusCode, body: &str) -> Response {
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Sign In - pnpr</title>
<style>{STYLE}</style>
</head>
<body>
<main>
<h1>Command-line sign-in</h1>
{body}
</main>
</body>
</html>"#,
    );
    let mut response = protect(
        (status, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response(),
    );
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY.clone());
    response
}

fn error_paragraph(error: Option<&str>) -> String {
    error
        .map(|error| format!("<p class=\"error\" role=\"alert\">{}</p>\n", escape_html(error)))
        .unwrap_or_default()
}

/// The page that asks for the code the CLI printed.
pub(super) fn code_page(status: StatusCode, error: Option<&str>) -> Response {
    let error = error_paragraph(error);
    html_page(
        status,
        &format!(
            r#"<p>Enter the code that your terminal shows. If you did not start a sign-in from a terminal, close this page.</p>
{error}<form method="post">
<label for="code">Code</label>
<input id="code" name="code" required autofocus autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="XXXX-XXXX">
<button type="submit">Continue</button>
</form>"#,
        ),
    )
}

/// The second page, for a pnpr with several OIDC login providers: the code
/// was accepted, and the user picks where to sign in. Only the browser that
/// typed the code receives it back here.
pub(super) fn provider_page(
    status: StatusCode,
    providers: &[&pnpr_config::oidc::OidcProvider],
    code: &str,
    error: Option<&str>,
) -> Response {
    let mut buttons = String::new();
    for provider in providers {
        let name = escape_html(&provider.name);
        let _ = write!(
            buttons,
            "\n<button type=\"submit\" name=\"provider\" value=\"{name}\">Sign in with {name}</button>",
        );
    }
    let code = escape_html(code);
    let error = error_paragraph(error);
    html_page(
        status,
        &format!(
            r#"<p>Choose where you sign in.</p>
{error}<form method="post">
<input type="hidden" name="code" value="{code}">{buttons}
</form>"#,
        ),
    )
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests;
