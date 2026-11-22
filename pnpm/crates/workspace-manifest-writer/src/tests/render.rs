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
    let out = AddedQuotes::detect(original).apply(edited);
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - \"a\"\n  - \"b\"\nallowBuilds:\n  '@scope/kept': true\n  \"@scope/added\": true\n  \"*\": true\n",
    );
}

#[test]
fn added_quotes_tell_an_added_scalar_after_an_equal_existing_one() {
    let original = "packages:\n  - \"a\"\n  - \"b\"\n  - '@x'\n";
    let edited = format!("{original}  - '@x'\n");
    let out = AddedQuotes::detect(original).apply(edited);
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(out, "packages:\n  - \"a\"\n  - \"b\"\n  - '@x'\n  - \"@x\"\n");
}

#[test]
fn added_quotes_tell_an_added_scalar_before_an_equal_existing_one() {
    let original = "packages:\n  - \"a\"\n  - \"b\"\nexisting: '@x'\n";
    let edited = "packages:\n  - \"a\"\n  - \"b\"\nadded: '@x'\nexisting: '@x'\n".to_string();
    let out = AddedQuotes::detect(original).apply(edited);
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(out, "packages:\n  - \"a\"\n  - \"b\"\nadded: \"@x\"\nexisting: '@x'\n");
}

#[test]
fn added_quotes_keep_single_quotes_where_double_quotes_need_escapes() {
    let original = "packages:\n  - \"a\"\n";
    let edited = format!("{original}  - '@x \"y\"'\n  - '@x\\y'\n");
    let out = AddedQuotes::detect(original).apply(edited.clone());
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(out, edited);
}

#[test]
fn added_quotes_leave_other_documents_unchanged() {
    for original in ["", "packages:\n  - a\n", "packages:\n  - 'a'\n", "a: \"x\"\nb: 'y'\n"] {
        let edited = format!("{original}c: '@x'\n");
        let out = AddedQuotes::detect(original).apply(edited.clone());
        eprintln!("OUTPUT:\n{out}\n");
        assert_eq!(out, edited);
    }
}
