use super::FailedToPublishError;
use pretty_assertions::assert_eq;

#[test]
fn single_line_body_is_appended_inline() {
    let err =
        FailedToPublishError::new("foo", "1.0.0", 403, "Forbidden".to_owned(), "nope".to_owned());
    assert_eq!(err.to_string(), "Failed to publish package foo@1.0.0 (status 403 Forbidden): nope");
}

#[test]
fn multi_line_body_is_indented_under_details() {
    let err =
        FailedToPublishError::new("foo", "1.0.0", 500, String::new(), "line1\nline2".to_owned());
    assert_eq!(
        err.to_string(),
        "Failed to publish package foo@1.0.0 (status 500)\nDetails:\n    line1\n    line2\n",
    );
}

#[test]
fn empty_body_leaves_only_the_summary() {
    let err =
        FailedToPublishError::new("foo", "1.0.0", 502, "Bad Gateway".to_owned(), "  ".to_owned());
    assert_eq!(err.to_string(), "Failed to publish package foo@1.0.0 (status 502 Bad Gateway)");
}
