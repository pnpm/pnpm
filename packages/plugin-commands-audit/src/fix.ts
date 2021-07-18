import { AuditReport, AuditAdvisory } from '@pnpm/audit'
import readProjectManifest from '@pnpm/read-project-manifest'
import fromPairs from 'ramda/src/fromPairs'

export default async function fix (dir: string, auditReport: AuditReport) {
  const { manifest, writeProjectManifest } = await readProjectManifest(dir)
  const vulnOverrides = createOverrides(Object.values(auditReport.advisories))
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

function createOverrides (advisories: AuditAdvisory[]) {
  return fromPairs(advisories.map((advisory) => [
    `${advisory.module_name}@${advisory.vulnerable_versions}`,
    advisory.patched_versions,
  ]))
}
