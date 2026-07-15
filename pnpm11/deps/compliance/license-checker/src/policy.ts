import { PnpmError } from '@pnpm/error'
import type { LicensesConfig } from '@pnpm/types'

import { isCompoundLicenseExpression } from './spdxMatcher.js'

export type NormalizedPolicy = LicensesConfig & { mode: 'strict' | 'loose' }

export function resolveLicensePolicy (licenses?: LicensesConfig | null): NormalizedPolicy | null {
  if (licenses == null || licenses.mode === 'none') {
    return null
  }
  const hasLists =
    (licenses.allowed?.length ?? 0) > 0 ||
    (licenses.disallowed?.length ?? 0) > 0 ||
    (licenses.overrides != null && Object.keys(licenses.overrides).length > 0)
  if (!hasLists) {
    return null
  }
  return { ...licenses, mode: licenses.mode ?? 'loose' }
}

// `pnpm licenses allow`/`disallow` reject compound (AND/OR) SPDX expressions
// at input (see editLicenseList.ts), but a hand-edited `pnpm-workspace.yaml`
// can put one directly into `licenses.allowed`/`licenses.disallowed`,
// bypassing that check. On the disallow side this is a silent fail-open: the
// matcher only compares single leaf candidates against the disallowed set,
// so a stored "GPL-3.0-only OR GPL-2.0-only" never matches any one leaf and
// nothing gets blocked. On the allow side a hand-edited compound is instead
// dropped by the matcher's `batchAllowed` filter (over-blocking, but silent).
// Reject both at scan time — before any scanning happens — with the same
// error the CLI uses, so this is caught regardless of how the policy entry
// got there.
export function assertNoCompoundPolicyEntries (policy: { allowed?: string[], disallowed?: string[] }): void {
  const badAllowed = (policy.allowed ?? []).filter(isCompoundLicenseExpression)
  const badDisallowed = (policy.disallowed ?? []).filter(isCompoundLicenseExpression)
  if (badAllowed.length === 0 && badDisallowed.length === 0) {
    return
  }
  const offenders: string[] = []
  if (badAllowed.length > 0) {
    offenders.push(`allowed: ${badAllowed.join(', ')}`)
  }
  if (badDisallowed.length > 0) {
    offenders.push(`disallowed: ${badDisallowed.join(', ')}`)
  }
  throw new PnpmError(
    'LICENSES_COMPOUND_EXPRESSION',
    `Compound license expressions (AND/OR) are not supported in the allowed/disallowed list: ${offenders.join('; ')}. ` +
    'List each license identifier separately, e.g. "pnpm licenses allow MIT Apache-2.0".'
  )
}
