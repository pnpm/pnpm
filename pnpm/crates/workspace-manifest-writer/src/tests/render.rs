use crate::render::{QuoteStyle, detect_quote_style, render_value_with_quotes};

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
fn render_value_with_quotes_formats_correctly() {
    assert_eq!(render_value_with_quotes("foo@1.0.0", QuoteStyle::Single), "foo@1.0.0");
    assert_eq!(render_value_with_quotes("foo@1.0.0", QuoteStyle::Double), "foo@1.0.0");
    assert_eq!(
        render_value_with_quotes("@better-auth/core@1.7.3", QuoteStyle::Single),
        "'@better-auth/core@1.7.3'",
    );
    assert_eq!(
        render_value_with_quotes("@better-auth/core@1.7.3", QuoteStyle::Double),
        r#""@better-auth/core@1.7.3""#,
    );
    assert_eq!(render_value_with_quotes("*", QuoteStyle::Single), "'*'");
    assert_eq!(render_value_with_quotes("*", QuoteStyle::Double), r#""*""#);
}
