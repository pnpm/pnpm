use super::{MAX_VIOLATIONS_TO_PRINT, RenderedViolation, VerifyError};

fn rendered(name: &str, version: &str, code: &'static str, reason: &str) -> RenderedViolation {
    RenderedViolation {
        name: name.to_string(),
        version: version.to_string(),
        code,
        reason: reason.to_string(),
    }
}

#[test]
fn single_min_age_violation_picks_min_age_variant() {
    let err = VerifyError::from_rendered(&[rendered(
        "acme",
        "1.0.0",
        "MINIMUM_RELEASE_AGE_VIOLATION",
        "was published yesterday",
    )]);
    assert!(matches!(err, VerifyError::MinimumReleaseAgeViolation { .. }), "got: {err:?}");
}

#[test]
fn single_trust_violation_picks_trust_variant() {
    let err = VerifyError::from_rendered(&[rendered(
        "acme",
        "1.0.0",
        "TRUST_DOWNGRADE",
        "evidence dropped",
    )]);
    assert!(matches!(err, VerifyError::TrustDowngrade { .. }), "got: {err:?}");
}

#[test]
fn mixed_codes_escalate_and_render_code_per_entry() {
    let err = VerifyError::from_rendered(&[
        rendered("acme", "1.0.0", "MINIMUM_RELEASE_AGE_VIOLATION", "young"),
        rendered("bravo", "2.0.0", "TRUST_DOWNGRADE", "downgrade"),
    ]);
    let VerifyError::LockfileResolutionVerification { count, breakdown } = err else {
        panic!("expected LockfileResolutionVerification");
    };
    assert_eq!(count, 2);
    assert!(breakdown.contains("[MINIMUM_RELEASE_AGE_VIOLATION]"));
    assert!(breakdown.contains("[TRUST_DOWNGRADE]"));
    assert!(breakdown.contains("acme@1.0.0"));
    assert!(breakdown.contains("bravo@2.0.0"));
}

#[test]
fn single_code_breakdown_omits_per_line_code() {
    let err = VerifyError::from_rendered(&[
        rendered("acme", "1.0.0", "MINIMUM_RELEASE_AGE_VIOLATION", "young"),
        rendered("bravo", "2.0.0", "MINIMUM_RELEASE_AGE_VIOLATION", "also young"),
    ]);
    let VerifyError::MinimumReleaseAgeViolation { breakdown, .. } = err else {
        panic!("expected MinimumReleaseAgeViolation");
    };
    assert!(!breakdown.contains("[MINIMUM_RELEASE_AGE_VIOLATION]"), "got: {breakdown}");
}

/// Without the trim, a poisoned lockfile would flood the terminal.
#[test]
fn over_cap_adds_and_n_more_summary() {
    let mut violations = Vec::new();
    let n = MAX_VIOLATIONS_TO_PRINT + 5;
    for i in 0..n {
        violations.push(rendered(
            &format!("pkg-{i}"),
            "1.0.0",
            "MINIMUM_RELEASE_AGE_VIOLATION",
            "young",
        ));
    }
    let VerifyError::MinimumReleaseAgeViolation { count, breakdown } =
        VerifyError::from_rendered(&violations)
    else {
        panic!("expected MinimumReleaseAgeViolation");
    };
    assert_eq!(count, n);
    assert!(breakdown.contains("…and 5 more"), "got: {breakdown}");
    let visible_lines = breakdown.lines().filter(|line| !line.starts_with("  …and")).count();
    assert_eq!(visible_lines, MAX_VIOLATIONS_TO_PRINT);
}

#[test]
fn single_entry_breakdown_has_no_trailing_newline() {
    let err = VerifyError::from_rendered(&[rendered(
        "acme",
        "1.0.0",
        "MINIMUM_RELEASE_AGE_VIOLATION",
        "young",
    )]);
    let VerifyError::MinimumReleaseAgeViolation { breakdown, .. } = err else {
        panic!("expected MinimumReleaseAgeViolation");
    };
    assert!(!breakdown.ends_with('\n'), "breakdown: {breakdown:?}");
}

/// Insta-snapshot pins the user-facing Display text so a future
/// format change surfaces as a reviewable diff.
#[test]
fn renders_single_entry_single_code() {
    let err = VerifyError::from_rendered(&[rendered(
        "acme",
        "1.0.0",
        "MINIMUM_RELEASE_AGE_VIOLATION",
        "was published at 2025-11-30T22:00:00.000Z, within the minimumReleaseAge cutoff (2025-11-30T00:00:00.000Z)",
    )]);
    insta::assert_snapshot!("single_entry_single_code", err.to_string());
}

#[test]
fn renders_three_entries_single_code() {
    let err = VerifyError::from_rendered(&[
        rendered(
            "acme",
            "1.0.0",
            "MINIMUM_RELEASE_AGE_VIOLATION",
            "was published at 2025-11-30T22:00:00.000Z, within the minimumReleaseAge cutoff (2025-11-30T00:00:00.000Z)",
        ),
        rendered(
            "bravo",
            "2.0.0",
            "MINIMUM_RELEASE_AGE_VIOLATION",
            "was published at 2025-11-30T22:30:00.000Z, within the minimumReleaseAge cutoff (2025-11-30T00:00:00.000Z)",
        ),
        rendered(
            "charlie",
            "3.0.0",
            "MINIMUM_RELEASE_AGE_VIOLATION",
            "was published at 2025-11-30T23:00:00.000Z, within the minimumReleaseAge cutoff (2025-11-30T00:00:00.000Z)",
        ),
    ]);
    insta::assert_snapshot!("three_entries_single_code", err.to_string());
}

#[test]
fn renders_three_entries_mixed_codes() {
    let err = VerifyError::from_rendered(&[
        rendered(
            "acme",
            "1.0.0",
            "MINIMUM_RELEASE_AGE_VIOLATION",
            "was published at 2025-11-30T22:00:00.000Z, within the minimumReleaseAge cutoff (2025-11-30T00:00:00.000Z)",
        ),
        rendered(
            "bravo",
            "2.0.0",
            "TRUST_DOWNGRADE",
            r#"High-risk trust downgrade for "bravo@2.0.0" (possible package takeover)"#,
        ),
        rendered(
            "charlie",
            "3.0.0",
            "MINIMUM_RELEASE_AGE_VIOLATION",
            "could not be checked against minimumReleaseAge (version not present in registry manifest)",
        ),
    ]);
    insta::assert_snapshot!("three_entries_mixed_codes", err.to_string());
}
