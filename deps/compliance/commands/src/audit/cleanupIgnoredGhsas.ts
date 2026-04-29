import type { AuditReport } from '@pnpm/deps.compliance.audit'
import { normalizeGhsaId } from '@pnpm/deps.compliance.audit'

export interface CleanupIgnoredGhsasResult {
  cleaned: string[]
  retained: string[]
}

export function cleanupIgnoredGhsas (
  ignoredGhsas: string[],
  auditReport: AuditReport
): CleanupIgnoredGhsasResult {
  if (!ignoredGhsas?.length) {
    return { cleaned: [], retained: [] }
  }

  const advisoryGhsaIds = new Set<string>(
    Object.values(auditReport.advisories)
      .filter(({ github_advisory_id: ghsaId }) => ghsaId)
      .map(({ github_advisory_id: ghsaId }) => normalizeGhsaId(ghsaId))
  )

  const retained = ignoredGhsas.filter((ghsa) => advisoryGhsaIds.has(normalizeGhsaId(ghsa)))
  const cleaned = ignoredGhsas.filter((ghsa) => !retained.includes(ghsa))

  return { cleaned, retained }
}
