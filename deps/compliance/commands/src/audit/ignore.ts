import { writeSettings } from '@pnpm/config.writer'
import { type AuditAdvisory, type AuditReport, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
import { PnpmError } from '@pnpm/error'
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
  // GHSA IDs are canonically uppercase; normalize on read/write so a stored
  // "ghsa-..." or uppercase user input both match the derived id at filter
  // time.
  const currentGhsas = (opts?.auditConfig?.ignoreGhsas ?? []).map(normalizeGhsaId)
  const currentUniqueGhsas = new Set(currentGhsas)
  const advisoriesWithNoResolutions = filterAdvisoriesWithNoResolutions(Object.values(opts.auditReport.advisories))

  if (opts.ignoreUnfixable) {
    for (const advisory of advisoriesWithNoResolutions) {
      if (!advisory.github_advisory_id) {
        throw new PnpmError(
          'AUDIT_MISSING_GHSA',
          `Cannot ignore advisory ${advisory.id} (${advisory.module_name}): the registry did not provide a GHSA id or a resolvable url.`
        )
      }
      currentUniqueGhsas.add(normalizeGhsaId(advisory.github_advisory_id))
    }
  } else if (opts.ignore) {
    for (const ghsa of opts.ignore) {
      currentUniqueGhsas.add(normalizeGhsaId(ghsa))
    }
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

// Advisories for which no override can be produced — patched_versions is
// undefined when pnpm couldn't infer a patched range from vulnerable_versions.
// That is the only "no fix available" signal the bulk endpoint provides.
function filterAdvisoriesWithNoResolutions (advisories: AuditAdvisory[]): AuditAdvisory[] {
  return advisories.filter(({ patched_versions: patchedVersions }) => patchedVersions == null)
}
