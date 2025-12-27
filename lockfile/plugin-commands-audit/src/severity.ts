import { VulnerabilitySeverity } from '@pnpm/types'
import { type AuditLevelString } from '@pnpm/audit'

export const AUDIT_LEVEL_SEVERITY = {
  low: VulnerabilitySeverity.low,
  moderate: VulnerabilitySeverity.moderate,
  high: VulnerabilitySeverity.high,
  critical: VulnerabilitySeverity.critical,
} satisfies Record<AuditLevelString, VulnerabilitySeverity>
