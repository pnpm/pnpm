import { type AuditReport, type AuditAdvisory } from '@pnpm/audit'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import difference from 'ramda/src/difference'

export async function fix (dir: string, auditReport: AuditReport): Promise<Record<string, string>> {
  const { manifest, writeProjectManifest } = await readProjectManifest(dir)
  const vulnOverrides = createOverrides(Object.values(auditReport.advisories), manifest.pnpm?.auditConfig?.ignoreCves)
  if (Object.values(vulnOverrides).length === 0) return vulnOverrides
  await writeProjectManifest({
    ...manifest,
    pnpm: {
      ...manifest.pnpm,
      overrides: {
        ...manifest.pnpm?.overrides,
        ...vulnOverrides,
      },
    },
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
