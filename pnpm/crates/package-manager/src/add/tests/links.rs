/// A `://` that no scheme precedes is not a URL authority, and a message
/// carrying no URL at all is passed through untouched.
#[test]
fn url_scrubbing_leaves_non_urls_alone() {
    for text in ["no url here at all", "why? because", "see :// for the syntax"] {
        assert_eq!(crate::add::aliasless::strip_url_query_and_fragment(text), text);
    }
}
/// An `@` past the authority belongs to the path or query, and must not
/// make an otherwise safe URL fail closed.
#[test]
fn url_scrubbing_keeps_a_path_that_contains_an_at_sign() {
    assert_eq!(
        crate::add::aliasless::strip_url_query_and_fragment(
            "GET https://host/@scope%2fpkg?to=a@b: 403"
        ),
        "GET https://host/@scope%2fpkg: 403",
    );
}
