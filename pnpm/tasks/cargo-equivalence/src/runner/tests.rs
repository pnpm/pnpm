use super::{Finding, Verdict, judge};
use crate::workspaces::Expectation;

const TRACKED: Expectation = Expectation::Differ { issue: "https://example.test/issue" };

#[test]
fn a_tracked_gap_accepts_only_a_disagreement() {
    let (verdict, message) = judge(TRACKED, Finding::Differ("cargo locked x".to_string()));

    eprintln!("MESSAGE: {message}");
    assert_eq!(verdict, Verdict::KnownDifference);
    assert!(message.contains("https://example.test/issue"));
}

/// A gap is a claim about what the resolvers produce, so it cannot
/// excuse a run that produced nothing to compare.
#[test]
fn a_tracked_gap_does_not_excuse_a_broken_run() {
    for broken in ["parse Cargo.lock", r#"run "pnpm": No such file"#] {
        let (verdict, message) = judge(TRACKED, Finding::Broken(broken.to_string()));

        eprintln!("MESSAGE: {message}");
        assert_eq!(verdict, Verdict::Unexpected, "{broken}");
    }
}

#[test]
fn a_tracked_gap_that_closed_is_reported() {
    let (verdict, message) = judge(TRACKED, Finding::Agree);

    eprintln!("MESSAGE: {message}");
    assert_eq!(verdict, Verdict::Unexpected);
    assert!(message.contains("looks fixed"));
}

#[test]
fn agreement_is_required_where_it_is_expected() {
    assert_eq!(judge(Expectation::Agree, Finding::Agree).0, Verdict::Agree);
    assert_eq!(
        judge(Expectation::Agree, Finding::Differ("cargo locked x".to_string())).0,
        Verdict::Unexpected,
    );
    assert_eq!(
        judge(Expectation::Agree, Finding::Broken("parse Cargo.lock".to_string())).0,
        Verdict::Unexpected,
    );
}
