//! Error surface for the lockfile-verification gate.
//!
//! Mirrors the three error codes pnpm raises from
//! [`buildVerificationError`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/verifyLockfileResolutions.ts#L172-L206):
//!
//! - `MINIMUM_RELEASE_AGE_VIOLATION` — every violation in the batch
//!   tripped the maturity check.
//! - `TRUST_DOWNGRADE` — every violation tripped the trust check.
//! - `LOCKFILE_RESOLUTION_VERIFICATION` — mixed batch (more than one
//!   distinct violation code). The per-entry code goes into the
//!   breakdown so the user can see which policy each entry tripped.
//!
//! The breakdown caps visible entries at 20 (matching upstream's
//! `MAX_VIOLATIONS_TO_PRINT`) and summarizes the remainder. Each
//! variant carries a `help` string verbatim from upstream so the
//! `pnpm errors` catalogue text matches.

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::fmt::Write as _;

/// Upstream's `MAX_VIOLATIONS_TO_PRINT`. Keeps a poisoned lockfile
/// from flooding the terminal with hundreds of rejection lines.
pub const MAX_VIOLATIONS_TO_PRINT: usize = 20;

const HINT: &str = "The lockfile contains entries that the active policies reject. \
This can mean the lockfile is stale, or that someone committed a \
lockfile that bypassed the policy locally — inspect recent changes \
to pnpm-lock.yaml before trusting it. If the changes look expected, \
run \"pnpm clean --lockfile\" and then \"pnpm install\" to rebuild from \
a fresh resolution. Alternatively, relax the policy that flagged \
them.";

/// One verifier rejection rendered for the error breakdown.
/// Internal-only data shape — the runner builds these from
/// `ResolutionPolicyViolation` after sorting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedViolation {
    pub name: String,
    pub version: String,
    pub code: &'static str,
    pub reason: String,
}

/// Errors raised by [`crate::verify_lockfile_resolutions()`]. Each
/// variant maps to the matching upstream `PnpmError` code. The
/// formatted message includes the count + per-entry breakdown,
/// trimmed to `MAX_VIOLATIONS_TO_PRINT`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum VerifyError {
    /// Every violation in the batch tripped
    /// `MINIMUM_RELEASE_AGE_VIOLATION`. Per-policy code preserved so
    /// existing handlers / docs route correctly.
    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION), help("{HINT}"))]
    MinimumReleaseAgeViolation {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },

    /// Every violation tripped `TRUST_DOWNGRADE`.
    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_TRUST_DOWNGRADE), help("{HINT}"))]
    TrustDowngrade {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },

    /// Every violation tripped `RESOLUTION_SHAPE_MISMATCH` — a
    /// registry-style dependency path backed by a non-registry
    /// resolution.
    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_RESOLUTION_SHAPE_MISMATCH), help("{HINT}"))]
    ResolutionShapeMismatch {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },

    /// Mixed batch — at least two distinct violation codes — so the
    /// throw code escalates to the generic
    /// `LOCKFILE_RESOLUTION_VERIFICATION` and each entry's code goes
    /// into the breakdown.
    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION), help("{HINT}"))]
    LockfileResolutionVerification {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },
}

impl VerifyError {
    /// Build the appropriate variant from a list of rendered
    /// violations. The list is **already sorted** by `name@version`
    /// (the runner sorts before calling). Empty input is a logic
    /// error — callers must check before constructing.
    #[must_use]
    pub fn from_rendered(violations: &[RenderedViolation]) -> Self {
        debug_assert!(!violations.is_empty(), "no violations → no error");
        let distinct_codes: std::collections::BTreeSet<&str> =
            violations.iter().map(|violation| violation.code).collect();
        let mixed = distinct_codes.len() > 1;
        let count = violations.len();
        let visible_count = count.min(MAX_VIOLATIONS_TO_PRINT);
        let omitted = count.saturating_sub(visible_count);

        let mut breakdown = String::new();
        for violation in violations.iter().take(visible_count) {
            if mixed {
                writeln!(
                    breakdown,
                    "  {name}@{version} [{code}] {reason}",
                    name = violation.name,
                    version = violation.version,
                    code = violation.code,
                    reason = violation.reason,
                )
                .unwrap();
            } else {
                writeln!(
                    breakdown,
                    "  {name}@{version} {reason}",
                    name = violation.name,
                    version = violation.version,
                    reason = violation.reason,
                )
                .unwrap();
            }
        }
        if omitted > 0 {
            write!(breakdown, "  …and {omitted} more").unwrap();
        } else if breakdown.ends_with('\n') {
            // Drop the final newline so the formatted error doesn't
            // carry trailing whitespace into log lines.
            breakdown.pop();
        }

        if mixed {
            VerifyError::LockfileResolutionVerification { count, breakdown }
        } else {
            // Safe: distinct_codes has exactly one element.
            let code = *distinct_codes.iter().next().expect("at least one code");
            match code {
                pacquet_resolving_npm_resolver_violation_codes::MINIMUM_RELEASE_AGE_VIOLATION => {
                    VerifyError::MinimumReleaseAgeViolation { count, breakdown }
                }
                pacquet_resolving_npm_resolver_violation_codes::TRUST_DOWNGRADE => {
                    VerifyError::TrustDowngrade { count, breakdown }
                }
                crate::RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE => {
                    VerifyError::ResolutionShapeMismatch { count, breakdown }
                }
                // Unknown verifier code (future-proofing): fall back
                // to the generic envelope rather than fabricating a
                // variant we don't have.
                _ => VerifyError::LockfileResolutionVerification { count, breakdown },
            }
        }
    }
}

/// Aliases the violation codes the npm verifier defines, so this
/// crate doesn't take a runtime dependency on
/// `pacquet-resolving-npm-resolver` just to compare two `&'static str`
/// constants. Keep the values byte-identical to the canonical
/// definitions over there.
mod pacquet_resolving_npm_resolver_violation_codes {
    /// Matches `pacquet_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE`.
    pub const MINIMUM_RELEASE_AGE_VIOLATION: &str = "MINIMUM_RELEASE_AGE_VIOLATION";
    /// Matches `pacquet_resolving_npm_resolver::TRUST_DOWNGRADE_VIOLATION_CODE`.
    pub const TRUST_DOWNGRADE: &str = "TRUST_DOWNGRADE";
}

#[cfg(test)]
mod tests;
