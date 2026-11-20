import { PnpmError } from '@pnpm/error'

export interface PnprViolation {
  name: string
  version: string
  code: string
  reason: string
}

const MAX_VIOLATIONS_TO_PRINT = 20

const KNOWN_VIOLATION_CODES = new Set([
  'MINIMUM_RELEASE_AGE_VIOLATION',
  'TRUST_DOWNGRADE',
  'TARBALL_URL_MISMATCH',
  'TARBALL_REVISION_MISMATCH',
  'MISSING_NAMED_REGISTRY',
  'MISSING_TARBALL_INTEGRITY',
  'RESOLUTION_SHAPE_MISMATCH',
])

const POLICY_VIOLATION_CODES = new Set([
  'MINIMUM_RELEASE_AGE_VIOLATION',
  'TRUST_DOWNGRADE',
])

const POLICY_HINT = 'The lockfile contains entries that the active policies reject. ' +
  'This can mean the lockfile is stale, or that someone committed a ' +
  'lockfile that bypassed the policy locally — inspect recent changes ' +
  'to pnpm-lock.yaml before trusting it. If the changes look expected, ' +
  'run "pnpm clean --lockfile" and then "pnpm install" to rebuild from ' +
  'a fresh resolution. If the fresh resolution still fails and you ' +
  'trust the affected packages, relax the policy that flagged them.'

const STRUCTURAL_HINT = 'The lockfile contains entries that pnpm cannot verify, whatever ' +
  'the configured policies. This can mean the lockfile is stale, or ' +
  'that it was tampered with — inspect recent changes to ' +
  'pnpm-lock.yaml before trusting it. If the changes look expected, ' +
  'run "pnpm clean --lockfile" and then "pnpm install" to rebuild ' +
  'from a fresh resolution.'

export function buildPnprVerifyError (violations: readonly PnprViolation[]): PnpmError {
  if (violations.length === 0) {
    return new PnpmError('LOCKFILE_RESOLUTION_VERIFICATION', 'pnpr server rejected the lockfile under the verification policy')
  }
  const normalized = violations
    .map((violation) => ({
      ...violation,
      code: KNOWN_VIOLATION_CODES.has(violation.code) ? violation.code : 'LOCKFILE_RESOLUTION_VERIFICATION',
    }))
    .sort((left, right) => `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`))
  const distinctCodes = new Set(normalized.map((violation) => violation.code))
  const isMixed = distinctCodes.size > 1
  const errorCode = isMixed ? 'LOCKFILE_RESOLUTION_VERIFICATION' : normalized[0].code
  const details = formatViolationDetails(normalized, isMixed)
  const hint = normalized.every((violation) => POLICY_VIOLATION_CODES.has(violation.code))
    ? POLICY_HINT
    : STRUCTURAL_HINT
  return new PnpmError(
    errorCode,
    `${normalized.length} lockfile entries failed verification:\n${details}`,
    { hint }
  )
}

function formatViolationDetails (violations: readonly PnprViolation[], isMixed: boolean): string {
  const visible = violations.slice(0, MAX_VIOLATIONS_TO_PRINT)
  const omitted = violations.length - visible.length
  const formatEntry = isMixed
    ? (violation: PnprViolation): string => `  ${violation.name}@${violation.version} [${violation.code}] ${violation.reason}`
    : (violation: PnprViolation): string => `  ${violation.name}@${violation.version} ${violation.reason}`
  const breakdown = visible.map(formatEntry).join('\n')
  return omitted > 0 ? `${breakdown}\n  …and ${omitted} more` : breakdown
}
