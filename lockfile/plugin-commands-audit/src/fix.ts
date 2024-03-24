import difference from 'ramda/src/difference'

import type { AuditReport, AuditAdvisory } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/read-project-manifest'

export async function fix(dir: string, auditReport: AuditReport): Promise<{
  [k: string]: string;
}> {
  const { manifest, writeProjectManifest } = await readProjectManifest(dir)

  const vulnOverrides = createOverrides(
    Object.values(auditReport.advisories),
    manifest.pnpm?.auditConfig?.ignoreCves
  )

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

function createOverrides(advisories: AuditAdvisory[], ignoreCves?: string[]): {
  [k: string]: string;
} {
  if (ignoreCves) {
    advisories = advisories.filter(
      ({ cves }: AuditAdvisory) => {
        return difference(cves, ignoreCves).length > 0;
      }
    )
  }

  return Object.fromEntries(
    advisories
      .filter(
        ({ vulnerable_versions, patched_versions }: AuditAdvisory): boolean => {
          return vulnerable_versions !== '>=0.0.0' && patched_versions !== '<0.0.0';
        }
      )
      .map((advisory: AuditAdvisory): [string, string] => [
        `${advisory.module_name}@${advisory.vulnerable_versions}`,
        advisory.patched_versions,
      ])
  )
}
