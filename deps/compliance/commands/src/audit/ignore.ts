import { writeSettings } from '@pnpm/config.writer'
import type { AuditAdvisory, AuditReport } from '@pnpm/deps.compliance.audit'
import type { AuditConfig, ProjectManifest } from '@pnpm/types'
import { difference } from 'ramda'

export interface IgnoreVulnerabilitiesOptions {
  dir: string
  ignore?: string[]
  ignoreUnfixable: boolean
  auditReport: AuditReport
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir: string
  auditConfig?: AuditConfig
}

export async function ignore (opts: IgnoreVulnerabilitiesOptions): Promise<string[]> {
  const currentGhsas = opts?.auditConfig?.ignoreGhsas ?? []
  const currentUniqueGhsas = new Set(currentGhsas)
  const advisoryWthNoResolutions = filterAdvisoriesWithNoResolutions(Object.values(opts.auditReport.advisories))

  if (opts.ignoreUnfixable) {
    Object.values(advisoryWthNoResolutions).forEach((advisory: AuditAdvisory) => {
      if (advisory.github_advisory_id) currentUniqueGhsas.add(advisory.github_advisory_id)
    })
  } else {
    opts.ignore?.forEach((ghsa) => currentUniqueGhsas.add(ghsa))
  }

  const newIgnoreGhsas = currentUniqueGhsas.size > 0 ? Array.from(currentUniqueGhsas) : undefined
  const diffGhsas = difference(newIgnoreGhsas ?? [], currentGhsas)
  await writeSettings({
    ...opts,
    updatedSettings: {
      auditConfig: {
        ...opts.auditConfig,
        ignoreGhsas: newIgnoreGhsas,
      },
    },
  })
  return [...diffGhsas]
}

function filterAdvisoriesWithNoResolutions (advisories: AuditAdvisory[]) {
  return advisories.filter(({ patched_versions: patchedVersions }) => patchedVersions === '<0.0.0' || patchedVersions === '')
}
