//! Resolution-policy violation codes.
//!
//! These constants are the verifier's piece of the public contract:
//! the install command filters violations by `code` to decide which
//! handler runs (auto-collect, strict-mode prompt, abort), and pnpm's
//! diagnostic catalog (<https://pnpm.io/errors>) routes the same
//! strings. The values are part of the public contract.

pub const MINIMUM_RELEASE_AGE_VIOLATION_CODE: &str = "MINIMUM_RELEASE_AGE_VIOLATION";
pub const TRUST_DOWNGRADE_VIOLATION_CODE: &str = "TRUST_DOWNGRADE";
pub const TARBALL_URL_MISMATCH_VIOLATION_CODE: &str = "TARBALL_URL_MISMATCH";
