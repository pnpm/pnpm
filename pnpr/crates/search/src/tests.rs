use super::{SearchParams, SearchText, parse_from, parse_params, parse_query, parse_size};

#[test]
fn parses_text_query() {
    assert_eq!(parse_query("text=is-positive&size=20").as_deref(), Some("is-positive"));
    assert_eq!(parse_query("size=20&text=foo").as_deref(), Some("foo"));
    assert_eq!(parse_query("text=hello%20world").as_deref(), Some("hello world"));
    assert_eq!(parse_query("text=hi+there").as_deref(), Some("hi there"));
    assert_eq!(parse_query("text=%40scope%2Fname").as_deref(), Some("@scope/name"));
}

#[test]
fn parses_q_fallback() {
    assert_eq!(parse_query("q=foo").as_deref(), Some("foo"));
}

#[test]
fn text_overrides_q_regardless_of_order() {
    assert_eq!(parse_query("q=fallback&text=primary").as_deref(), Some("primary"));
    assert_eq!(parse_query("text=primary&q=fallback").as_deref(), Some("primary"));
}

#[test]
fn no_query() {
    assert!(parse_query("").is_none());
    assert!(parse_query("size=20").is_none());
}

#[test]
fn empty_text_is_no_query() {
    // An empty needle would make `contains("")` true downstream and
    // dump every package in storage. Treat `text=` the same as no
    // text at all.
    assert!(parse_query("text=").is_none());
    assert!(parse_query("text=&size=20").is_none());
}

#[test]
fn malformed_pair_doesnt_abort_parse() {
    // A pair with no `=` (e.g. trailing `&` or an unkeyed value)
    // used to short-circuit the whole parse with `?`. Now we just
    // skip it.
    assert_eq!(parse_query("flag&text=foo").as_deref(), Some("foo"));
    assert_eq!(parse_query("text=foo&trailing").as_deref(), Some("foo"));
}

#[test]
fn size_clamps() {
    assert_eq!(parse_size("size=10", 20), 10);
    assert_eq!(parse_size("size=0", 20), 1);
    assert_eq!(parse_size("size=9999", 20), 250);
    assert_eq!(parse_size("size=garbage", 20), 20);
    assert_eq!(parse_size("", 20), 20);
}

#[test]
fn parses_pagination() {
    assert_eq!(parse_from("text=foo&from=25&size=10"), 25);
    assert_eq!(parse_from("text=foo&from=invalid"), 0);
    assert_eq!(parse_from("text=foo"), 0);
}

#[test]
fn parses_package_search_params() {
    assert_eq!(
        parse_params("text=foo&from=20&size=10", 20),
        Some(SearchParams { text: SearchText::Package("foo".to_string()), from: 20, size: 10 }),
    );
}

#[test]
fn parses_maintainer_search_params() {
    assert_eq!(
        parse_params("text=maintainer%3Aalice&size=1", 20),
        Some(SearchParams { text: SearchText::Maintainer("alice".to_string()), from: 0, size: 1 }),
    );
}
