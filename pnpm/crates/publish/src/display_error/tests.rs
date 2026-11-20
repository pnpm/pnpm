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

#[test]
fn empty_strings_are_treated_as_absent() {
    assert_eq!(display_error(Some(""), Some("boom")), "boom");
    assert_eq!(display_error(Some("ERR_X"), Some("")), "ERR_X");
    assert_eq!(display_error(Some(""), Some("")), "null");
}
