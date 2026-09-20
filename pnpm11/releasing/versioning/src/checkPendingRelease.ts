import {
  assembleReleasePlan,
  checkVersioningInvariants,
  type CheckVersioningInvariantsOptions,
  type VersioningInvariantViolation,
} from './assembleReleasePlan.js'
import { readChangeIntents } from './intents.js'
import { readLedger } from './ledger.js'

export interface PendingReleaseCheck {
  /** How many intent files `.changeset/` holds. */
  intentCount: number
  violations: VersioningInvariantViolation[]
}

/**
 * What a release run validates before it needs the registry: the pending
 * change intents resolve to packages this workspace can release, the
 * `versioning` configuration they run through is well-formed, and the
 * committed versions still satisfy the invariants it declares. A malformed
 * intent or configuration throws, as it would at release time; drifted
 * versions come back as violations for the caller to list. Internal
 * dependencies still on a plain range are left alone, as they are for
 * `pnpm change status`.
 */
export async function checkPendingRelease (opts: CheckVersioningInvariantsOptions): Promise<PendingReleaseCheck> {
  const intents = await readChangeIntents(opts.workspaceDir)
  const ledger = await readLedger(opts.workspaceDir)
  assembleReleasePlan({ ...opts, intents, ledger })
  return {
    intentCount: intents.length,
    violations: checkVersioningInvariants(opts),
  }
}

/** The one-line summary of what {@link checkPendingRelease} validated. */
export function describeCheckedIntents (intentCount: number): string {
  if (intentCount === 0) return 'No pending change intents to check.'
  return `Checked ${intentCount} pending change intent${intentCount === 1 ? '' : 's'}: every one names a releasable package.`
}
