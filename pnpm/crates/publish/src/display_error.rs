//! render a caught error as a short one-line string
//! for a warning message.

use pacquet_diagnostics::miette::Diagnostic;

/// Combine an error's `code` (or `name`) and `message` into a one-line
/// string:
///
/// - both present → `"<code>: <body>"`
/// - only one present → that one
/// - neither → `"null"` (the JS fallback is `JSON.stringify(error)`; the
///   call sites that reach this never carry an arbitrary object, so the
///   degenerate case only needs a stable placeholder).
#[must_use]
pub fn display_error(code: Option<&str>, body: Option<&str>) -> String {
    // TS treats empty strings as absent (`error.code` / `error.message` are
    // truthy-tested), so normalize `Some("")` to `None` before matching.
    let code = code.filter(|code| !code.is_empty());
    let body = body.filter(|body| !body.is_empty());
    match (code, body) {
        (Some(code), Some(body)) => format!("{code}: {body}"),
        (Some(code), None) => code.to_owned(),
        (None, Some(body)) => body.to_owned(),
        (None, None) => "null".to_owned(),
    }
}

/// Render a diagnostic as a short string: its `code` followed by its
/// message. The miette `code` supplies the code and `Display` the message.
pub(crate) fn display_diagnostic(error: &impl Diagnostic) -> String {
    let code = error.code().map(|code| code.to_string());
    display_error(code.as_deref(), Some(&error.to_string()))
}

#[cfg(test)]
mod tests;
