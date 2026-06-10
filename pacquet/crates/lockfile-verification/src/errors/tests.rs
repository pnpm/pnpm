use super::{MAX_VIOLATIONS_TO_PRINT, RenderedViolation, VerifyError};

fn rendered(name: &str, version: &str, code: &'static str, reason: &str) -> RenderedViolation {
    RenderedViolation {
        name: name.to_string(),
        version: version.to_string(),
        code,
        reason: reason.to_string(),
    }
}

/// A batch where every violation tripped `MINIMUM_RELEASE_AGE_VIOLATION`
/// resolves to the per-policy variant; existing error handlers /
/// docs that match on the code still route correctly.
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

/// A trust-only batch picks the trust variant.
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

/// A batch with two distinct codes escalates to the generic
/// `LOCKFILE_RESOLUTION_VERIFICATION` variant; the per-entry code
/// shows up in the breakdown line.
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

/// Single-code batches do NOT include the code in each line — the
/// envelope's `code` carries that information.
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

/// More than `MAX_VIOLATIONS_TO_PRINT` entries trims the visible
/// list and adds the `…and N more` summary line. Without the trim, a
/// poisoned lockfile would flood the terminal.
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
    // The breakdown shows exactly MAX visible lines plus the summary.
    let visible_lines = breakdown.lines().filter(|line| !line.starts_with("  …and")).count();
    assert_eq!(visible_lines, MAX_VIOLATIONS_TO_PRINT);
}

/// A 1-entry batch builds an error with no trailing newline in the
/// breakdown — matters for clean log lines.
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

/// One-entry single-code rendering — the canonical "the user
/// committed a lockfile with one immature pin" shape. Insta-snapshot
/// pins the user-facing Display text so a future format change
/// surfaces as a reviewable diff. Mirrors pnpm's `PnpmError` Display
/// shape; the per-policy code lives on the envelope, the breakdown
/// is single-column.
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

/// Three-entry single-code rendering — every entry tripped the same
/// policy. The envelope's `code` carries the policy; the breakdown
/// lists `<name>@<version> <reason>` without per-line code prefixes.
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

/// Three-entry mixed-code rendering — at least two distinct
/// violation codes in the batch. The envelope escalates to
/// `LOCKFILE_RESOLUTION_VERIFICATION`; the breakdown carries the
/// per-line code prefix so the user can see which policy each entry
/// tripped.
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
