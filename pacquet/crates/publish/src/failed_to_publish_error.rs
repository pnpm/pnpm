//! Port of `FailedToPublishError.ts`: the error raised when the registry
//! rejects a publish request.

use std::fmt::Write;

use pacquet_diagnostics::miette::{self, Diagnostic};

/// The registry returned a non-OK response for a publish. Ports pnpm's
/// `FailedToPublishError` (`ERR_PNPM_FAILED_TO_PUBLISH`).
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("{message}")]
#[diagnostic(code(ERR_PNPM_FAILED_TO_PUBLISH))]
pub struct FailedToPublishError {
    message: String,
    pub status: u16,
    pub status_text: String,
    pub text: String,
}

impl FailedToPublishError {
    /// Build the error from the rejected response and the package identity.
    /// Mirrors the TS constructor's message assembly: a one-line summary, with
    /// a multi-line response body rendered as an indented `Details:` block and
    /// a single-line body appended inline.
    #[must_use]
    pub fn new(name: &str, version: &str, status: u16, status_text: String, text: String) -> Self {
        let status_display = if status_text.is_empty() {
            status.to_string()
        } else {
            format!("{status} {status_text}")
        };

        let trimmed = text.trim();
        let mut message =
            format!("Failed to publish package {name}@{version} (status {status_display})");
        if trimmed.contains('\n') {
            message.push_str("\nDetails:\n");
            for line in text.trim_end().split('\n') {
                let _ = writeln!(message, "    {line}");
            }
        } else if !trimmed.is_empty() {
            let _ = write!(message, ": {trimmed}");
        }

        FailedToPublishError { message, status, status_text, text }
    }
}

#[cfg(test)]
mod tests {
    use super::FailedToPublishError;
    use pretty_assertions::assert_eq;

    #[test]
    fn single_line_body_is_appended_inline() {
        let err = FailedToPublishError::new(
            "foo",
            "1.0.0",
            403,
            "Forbidden".to_owned(),
            "nope".to_owned(),
        );
        assert_eq!(
            err.to_string(),
            "Failed to publish package foo@1.0.0 (status 403 Forbidden): nope"
        );
    }

    #[test]
    fn multi_line_body_is_indented_under_details() {
        let err = FailedToPublishError::new(
            "foo",
            "1.0.0",
            500,
            String::new(),
            "line1\nline2".to_owned(),
        );
        assert_eq!(
            err.to_string(),
            "Failed to publish package foo@1.0.0 (status 500)\nDetails:\n    line1\n    line2\n"
        );
    }

    #[test]
    fn empty_body_leaves_only_the_summary() {
        let err = FailedToPublishError::new(
            "foo",
            "1.0.0",
            502,
            "Bad Gateway".to_owned(),
            "  ".to_owned(),
        );
        assert_eq!(err.to_string(), "Failed to publish package foo@1.0.0 (status 502 Bad Gateway)");
    }
}
