import { type AuditReport, type AuditAdvisory } from '@pnpm/audit'
import { writeSettings } from '@pnpm/config.config-writer'
import difference from 'ramda/src/difference'
import { type AuditOptions } from './audit'

export async function fix (auditReport: AuditReport, opts: AuditOptions): Promise<Record<string, string>> {
  const vulnOverrides = createOverrides(Object.values(auditReport.advisories), opts.auditConfig?.ignoreCves)
  if (Object.values(vulnOverrides).length === 0) return vulnOverrides
  await writeSettings({
    updatedSettings: {
      overrides: {
        ...opts.overrides,
        ...vulnOverrides,
      },
    },
    rootProjectManifest: opts.rootProjectManifest,
    rootProjectManifestDir: opts.rootProjectManifestDir,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })
  return vulnOverrides
}

function createOverrides (advisories: AuditAdvisory[], ignoreCves?: string[]): Record<string, string> {
  if (ignoreCves) {
    advisories = advisories.filter(({ cves }) => difference(cves, ignoreCves).length > 0)
  }
  return Object.fromEntries(
    advisories
      .filter(({ vulnerable_versions, patched_versions }) => vulnerable_versions !== '>=0.0.0' && patched_versions !== '<0.0.0') // eslint-disable-line
      .map((advisory) => [
        `${advisory.module_name}@${advisory.vulnerable_versions}`,
        advisory.patched_versions,
      ])
  )
}
