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

  const retained: string[] = []
  const cleaned: string[] = []
  for (const ghsa of ignoredGhsas) {
    if (advisoryGhsaIds.has(normalizeGhsaId(ghsa))) {
      retained.push(ghsa)
    } else {
      cleaned.push(ghsa)
    }
  }

  return { cleaned, retained }
}
