//! Port of `displayError`: render a caught error as a short one-line string
//! for a warning message.

use pacquet_diagnostics::miette::Diagnostic;

/// Combine an error's `code` (or `name`) and `message` into a one-line
/// string, mirroring TS `displayError`:
///
/// - both present → `"<code>: <body>"`
/// - only one present → that one
/// - neither → `"null"` (the JS fallback is `JSON.stringify(error)`; the
///   call sites that reach this never carry an arbitrary object, so the
///   degenerate case only needs a stable placeholder).
#[must_use]
pub fn display_error(code: Option<&str>, body: Option<&str>) -> String {
    match (code, body) {
        (Some(code), Some(body)) => format!("{code}: {body}"),
        (Some(code), None) => code.to_owned(),
        (None, Some(body)) => body.to_owned(),
        (None, None) => "null".to_owned(),
    }
}

/// Render a diagnostic the way TS `displayError` renders a `PnpmError`: its
/// `code` followed by its message. The miette `code` plays the role of the
/// JS error's `.code`, and `Display` plays the role of `.message`.
pub(crate) fn display_diagnostic(error: &impl Diagnostic) -> String {
    let code = error.code().map(|code| code.to_string());
    display_error(code.as_deref(), Some(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::display_error;
    use pretty_assertions::assert_eq;

    #[test]
    fn combines_code_and_body() {
        assert_eq!(display_error(Some("ERR_X"), Some("boom")), "ERR_X: boom");
    }

    #[test]
    fn falls_back_to_whichever_is_present() {
        assert_eq!(display_error(Some("ERR_X"), None), "ERR_X");
        assert_eq!(display_error(None, Some("boom")), "boom");
    }

    #[test]
    fn placeholder_when_neither_is_present() {
        assert_eq!(display_error(None, None), "null");
    }
}
