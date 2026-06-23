//! Error surface for the lockfile-verification gate.

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

const INVALID_ALIAS_HINT: &str = "A dependency alias becomes a directory under node_modules, \
so it must be a valid npm package name — a single `name` or `@scope/name` with no leading \
`.` or `_`, and not a reserved name such as `node_modules`. An alias containing path-traversal \
segments or a reserved name such as `.bin` or `.pnpm` could make an install write outside the \
intended directory or overwrite pnpm-owned layout. This usually means the lockfile was tampered \
with — inspect recent changes to pnpm-lock.yaml before trusting it.";

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
    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION), help("{HINT}"))]
    MinimumReleaseAgeViolation {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },

    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_TRUST_DOWNGRADE), help("{HINT}"))]
    TrustDowngrade {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },

    /// The registry couldn't be reached to verify an entry
    /// (auth/network/5xx). Surfaces the registry's own fetch error — which
    /// already explains the auth situation — rather than a tampering-style
    /// mismatch or a lockfile-policy batch. The message is credential-redacted
    /// at the verifier before it reaches here.
    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_META_FETCH_FAIL))]
    RegistryMetaFetchFailed {
        #[error(not(source))]
        message: String,
    },

    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_RESOLUTION_SHAPE_MISMATCH), help("{HINT}"))]
    ResolutionShapeMismatch {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },

    #[display("{count} lockfile entries failed verification:\n{breakdown}")]
    #[diagnostic(code(ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION), help("{HINT}"))]
    LockfileResolutionVerification {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },

    /// One or more dependency aliases in the lockfile are not valid npm
    /// package names. Surfaces `ERR_PNPM_INVALID_DEPENDENCY_NAME`, the
    /// same code the sink-level guards raise.
    #[display("{count} dependency {plural} in the lockfile {verb} not valid package names:\n{breakdown}", plural = if *count == 1 { "alias" } else { "aliases" }, verb = if *count == 1 { "is" } else { "are" })]
    #[diagnostic(code(ERR_PNPM_INVALID_DEPENDENCY_NAME), help("{INVALID_ALIAS_HINT}"))]
    InvalidDependencyAlias {
        #[error(not(source))]
        count: usize,
        breakdown: String,
    },
}

impl VerifyError {
    /// Build the [`VerifyError::InvalidDependencyAlias`] variant from a
    /// list of offending aliases. Sorts for determinism and caps the
    /// printed breakdown at `MAX_VIOLATIONS_TO_PRINT`. Empty input is
    /// a logic error — callers must check before constructing.
    #[must_use]
    pub fn invalid_dependency_aliases(aliases: &[String]) -> Self {
        debug_assert!(!aliases.is_empty(), "no invalid aliases → no error");
        let mut sorted: Vec<&String> = aliases.iter().collect();
        sorted.sort();
        let count = sorted.len();
        let visible_count = count.min(MAX_VIOLATIONS_TO_PRINT);
        let omitted = count.saturating_sub(visible_count);

        let mut breakdown = String::new();
        for alias in sorted.iter().take(visible_count) {
            writeln!(breakdown, "  {alias:?}").unwrap();
        }
        if omitted > 0 {
            write!(breakdown, "  …and {omitted} more").unwrap();
        } else if breakdown.ends_with('\n') {
            breakdown.pop();
        }

        VerifyError::InvalidDependencyAlias { count, breakdown }
    }

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
