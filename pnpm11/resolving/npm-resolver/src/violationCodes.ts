/**
 * Violation codes the npm resolver attaches to
 * `ResolutionPolicyViolation.code` when an inline policy check rejects
 * a pick. Exported so downstream code (the install command, the strict
 * resolver wrapper, tests) references one source of truth instead of
 * re-typing the string.
 *
 * Lives in its own module — both `index.ts` and `createNpmResolutionVerifier.ts`
 * import it, so keeping the constants here avoids a cycle.
 */
export const MINIMUM_RELEASE_AGE_VIOLATION_CODE = 'MINIMUM_RELEASE_AGE_VIOLATION'
export const TRUST_DOWNGRADE_VIOLATION_CODE = 'TRUST_DOWNGRADE'
export const TARBALL_URL_MISMATCH_VIOLATION_CODE = 'TARBALL_URL_MISMATCH'
export const MISSING_TARBALL_INTEGRITY_VIOLATION_CODE = 'MISSING_TARBALL_INTEGRITY'
