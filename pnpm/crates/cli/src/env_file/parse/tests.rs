use super::{ParseError, Parser};
use std::{cell::RefCell, collections::HashMap};

/// Parse `content` the way the loader does: each entry is visible to the
/// entries after it, and `environment` stands in for the process
/// environment.
fn parse(content: &str, environment: &[(&str, &str)]) -> Result<Vec<(String, String)>, ParseError> {
    let defined: HashMap<String, String> = environment
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let defined = RefCell::new(defined);
    let mut parser = Parser::new(content);
    let mut entries = Vec::new();
    while let Some((key, value)) = parser.next_entry(|name| defined.borrow().get(name).cloned())? {
        defined.borrow_mut().insert(key.clone(), value.clone());
        entries.push((key, value));
    }
    Ok(entries)
}

/// The value of the single entry in `content`.
fn value_of(content: &str) -> String {
    let entries = parse(content, &[]).expect("parse env file");
    assert_eq!(entries.len(), 1, "expected one entry: {entries:?}");
    entries.into_iter().next().unwrap().1
}

fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[test]
fn expands_names_with_underscores() {
    let entries = parse("T_BASE=foo\nT_A=$T_BASE-bar\nT_B=${T_BASE}-baz\n", &[]);
    let expected = pairs(&[("T_BASE", "foo"), ("T_A", "foo-bar"), ("T_B", "foo-baz")]);
    assert_eq!(entries, Ok(expected));
}

#[test]
fn expands_from_the_environment() {
    let entries = parse("T_A=\"$HOME_DIR/cache\"\n", &[("HOME_DIR", "/home/me")]);
    assert_eq!(entries, Ok(pairs(&[("T_A", "/home/me/cache")])));
}

#[test]
fn expands_an_undefined_name_to_an_empty_string() {
    assert_eq!(value_of("T_A=pa$zzword\n"), "pa");
}

#[test]
fn keeps_a_dollar_that_starts_no_name() {
    assert_eq!(value_of("T_A=\"cost $5\"\n"), "cost $5");
    assert_eq!(value_of("T_A=a$\n"), "a$");
    assert_eq!(value_of("T_A=$-x\n"), "$-x");
}

#[test]
fn keeps_backslashes_that_are_not_escapes() {
    assert_eq!(value_of("T_A=C:\\Users\\me\\data\n"), r"C:\Users\me\data");
    assert_eq!(value_of("T_A=\"C:\\Users\\me\"\n"), r"C:\Users\me");
}

#[test]
fn keeps_unquoted_values_as_written() {
    assert_eq!(value_of("T_A=hello world\n"), "hello world");
    assert_eq!(value_of("T_A={\"k\":\"v\"}\n"), r#"{"k":"v"}"#);
    assert_eq!(value_of("T_A=it's\n"), "it's");
    assert_eq!(value_of("T_A=  padded  \n"), "padded");
}

#[test]
fn cuts_unquoted_values_at_a_comment() {
    assert_eq!(value_of("T_A=value # comment\n"), "value");
    assert_eq!(value_of("T_A=value#not-a-comment\n"), "value#not-a-comment");
    assert_eq!(value_of("T_A=#comment\n"), "");
}

#[test]
fn reads_double_quoted_escapes() {
    let content = "T_A=\"a\\nb\\tc \\\"q\\\" \\$HOME \\\\\"\n";
    assert_eq!(value_of(content), "a\nb\tc \"q\" $HOME \\");
}

#[test]
fn keeps_single_quoted_values_literal() {
    assert_eq!(value_of("T_A='$HOME \\n \"x\"'\n"), r#"$HOME \n "x""#);
}

#[test]
fn reads_quoted_values_across_lines() {
    let entries = parse("T_A=\"line 1\nline 2\"\nT_B='a\nb' # comment\nT_C=c\n", &[]);
    let expected = pairs(&[("T_A", "line 1\nline 2"), ("T_B", "a\nb"), ("T_C", "c")]);
    assert_eq!(entries, Ok(expected));
}

#[test]
fn skips_blank_lines_comments_and_export_prefixes() {
    let content = "\r\n# comment\r\n  \r\nexport T_A=1\r\n  T_B = 2\r\nT.C=3";
    assert_eq!(parse(content, &[]), Ok(pairs(&[("T_A", "1"), ("T_B", "2"), ("T.C", "3")])));
}

#[test]
fn reads_export_as_a_key_when_an_equals_sign_follows() {
    assert_eq!(
        parse("export = 1\nexport\t=2\n", &[]),
        Ok(pairs(&[("export", "1"), ("export", "2")])),
    );
}

#[test]
fn reads_empty_values() {
    assert_eq!(
        parse("T_A=\nT_B=''\nT_C=\"\"\n", &[]),
        Ok(pairs(&[("T_A", ""), ("T_B", ""), ("T_C", "")])),
    );
}

#[test]
fn reports_the_line_of_a_malformed_entry() {
    let cases = [
        ("T_A=1\n\nno equals here\n", 3),
        ("T_A=1\nT-B=2\n", 2),
        ("T_A=\"never closed\nT_B=2\n", 1),
        ("T_A=1\nT_B='never closed\n", 2),
        ("T_A='quoted' trailing\n", 1),
        ("T_A=${T_B\n", 1),
        ("T_A=${1B}\n", 1),
        ("T_A=${}\n", 1),
        ("T_A=\"a\nb\"\nbad\n", 3),
    ];
    for (content, line) in cases {
        assert_eq!(parse(content, &[]), Err(ParseError { line }), "content: {content:?}");
    }
}
