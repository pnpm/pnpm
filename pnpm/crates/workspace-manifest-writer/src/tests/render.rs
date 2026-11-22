use crate::render::{
    AddedQuotes,
    QuoteStyle,
    detect_quote_style,
};

#[test]
fn detect_quote_style_defaults_to_single_when_empty_or_unquoted() {
    assert_eq!(detect_quote_style(""), QuoteStyle::Single);
    assert_eq!(detect_quote_style("packages:\n  - pkgs/*\n"), QuoteStyle::Single);
}

#[test]
fn detect_quote_style_detects_single_and_double_quotes() {
    assert_eq!(detect_quote_style("packages:\n  - 'packages/*'\n"), QuoteStyle::Single);
    assert_eq!(detect_quote_style("packages:\n  - \"packages/*\"\n"), QuoteStyle::Double);
    assert_eq!(detect_quote_style("trustPolicy: \"no-downgrade\"\n"), QuoteStyle::Double);
}

#[test]
fn detect_quote_style_picks_dominant_style() {
    let double_dominant = "packages:\n  - \"a\"\n  - \"b\"\ncatalog:\n  x: '1'\n";
    assert_eq!(detect_quote_style(double_dominant), QuoteStyle::Double);

    let single_dominant = "packages:\n  - 'a'\n  - 'b'\ncatalog:\n  x: \"1\"\n";
    assert_eq!(detect_quote_style(single_dominant), QuoteStyle::Single);
}

#[test]
fn detect_quote_style_falls_back_when_equal() {
    let equal = "packages:\n  - \"a\"\ncatalog:\n  x: '1'\n";
    assert_eq!(detect_quote_style(equal), QuoteStyle::Single);
}

#[test]
fn detect_quote_style_ignores_comments() {
    let yaml = "# \"double\" 'single' in comments\npackages:\n  - 'packages/*'\n";
    assert_eq!(detect_quote_style(yaml), QuoteStyle::Single);
}

#[test]
fn added_quotes_double_quote_added_scalars_in_a_double_quoted_document() {
    let original = "packages:\n  - \"a\"\n  - \"b\"\nallowBuilds:\n  '@scope/kept': true\n";
    let edited = format!("{original}  '@scope/added': true\n  '*': true\n");
    assert_eq!(
        AddedQuotes::detect(original).apply(edited),
        "packages:\n  - \"a\"\n  - \"b\"\nallowBuilds:\n  '@scope/kept': true\n  \"@scope/added\": true\n  \"*\": true\n",
    );
}

#[test]
fn added_quotes_count_existing_single_quoted_scalars() {
    let original = "packages:\n  - \"a\"\n  - \"b\"\n  - '@x'\n";
    let edited = format!("{original}  - '@x'\n");
    assert_eq!(
        AddedQuotes::detect(original).apply(edited),
        "packages:\n  - \"a\"\n  - \"b\"\n  - '@x'\n  - \"@x\"\n",
    );
}

#[test]
fn added_quotes_keep_single_quotes_where_double_quotes_need_escapes() {
    let original = "packages:\n  - \"a\"\n";
    let edited = format!("{original}  - '@x \"y\"'\n  - '@x\\y'\n");
    assert_eq!(AddedQuotes::detect(original).apply(edited.clone()), edited);
}

#[test]
fn added_quotes_leave_other_documents_unchanged() {
    for original in ["", "packages:\n  - a\n", "packages:\n  - 'a'\n", "a: \"x\"\nb: 'y'\n"] {
        let edited = format!("{original}c: '@x'\n");
        assert_eq!(AddedQuotes::detect(original).apply(edited.clone()), edited);
    }
}
