import type { AuditReport } from '@pnpm/deps.compliance.audit'
import { normalizeGhsaId } from '@pnpm/deps.compliance.audit'

export interface PruneIgnoredGhsasResult {
  pruned: string[]
  retained: string[]
}

export function pruneIgnoredGhsas (
  ignoredGhsas: string[],
  auditReport: AuditReport
): PruneIgnoredGhsasResult {
  if (!ignoredGhsas?.length) {
    return { pruned: [], retained: [] }
  }

  const advisoryGhsaIds = new Set<string>(
    Object.values(auditReport.advisories)
      .filter(({ github_advisory_id: ghsaId }) => ghsaId)
      .map(({ github_advisory_id: ghsaId }) => normalizeGhsaId(ghsaId))
  )

  const retainedGhsas = new Set<string>()
  const pruned: string[] = []
  for (const ghsa of ignoredGhsas) {
    const normalized = normalizeGhsaId(ghsa)
    if (advisoryGhsaIds.has(normalized)) {
      retainedGhsas.add(normalized)
    } else {
      pruned.push(ghsa)
    }
  }

  return { pruned, retained: Array.from(retainedGhsas) }
}
