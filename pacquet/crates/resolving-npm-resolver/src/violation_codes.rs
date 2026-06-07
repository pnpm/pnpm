//! Verbatim port of pnpm's
//! [`violationCodes.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/violationCodes.ts).
//!
//! These constants are the verifier's piece of the public contract:
//! the install command filters violations by `code` to decide which
//! handler runs (auto-collect, strict-mode prompt, abort), and pnpm's
//! diagnostic catalog (<https://pnpm.io/errors>) routes the same
//! strings. Keep the values byte-identical to upstream.

pub const MINIMUM_RELEASE_AGE_VIOLATION_CODE: &str = "MINIMUM_RELEASE_AGE_VIOLATION";
pub const TRUST_DOWNGRADE_VIOLATION_CODE: &str = "TRUST_DOWNGRADE";
pub const TARBALL_URL_MISMATCH_VIOLATION_CODE: &str = "TARBALL_URL_MISMATCH";
