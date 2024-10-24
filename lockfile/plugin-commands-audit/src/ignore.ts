import { type AuditAdvisory, type AuditReport } from '@pnpm/audit'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import groupBy from 'ramda/src/groupBy'
import difference from 'ramda/src/difference'

export async function ignore (dir: string, commaDelimList: string, auditReport: AuditReport): Promise<string[]> {
  const { manifest, writeProjectManifest } = await readProjectManifest(dir)
  let ignoreAllCve = false
  const ignoreCveUserList = groupBy((s) => {
    if (s.startsWith('CVE-')) return 'CVE'
    return 'UNKNOWN'
  }, commaDelimList.split(','))

  if (!ignoreCveUserList['CVE']) ignoreAllCve = true

  const currentCves = manifest.pnpm?.auditConfig?.ignoreCves ?? []
  const currentUniqueCves = new Set(currentCves)
  const advisoryWthNoResolutions = filterAdvisoriesWithNoResolutions(Object.values(auditReport.advisories))

  if (ignoreAllCve) {
    Object.values(advisoryWthNoResolutions).forEach((advisory: AuditAdvisory) => {
      advisory.cves.forEach((cve) => currentUniqueCves.add(cve))
    })
  } else {
    ignoreCveUserList['CVE']?.forEach((cve) => currentUniqueCves.add(cve))
  }

  const config = {
    ignoreCves: currentUniqueCves.size > 0 ? Array.from(currentUniqueCves) : undefined,
  }
  const diffCve = difference(config.ignoreCves ?? [], currentCves)
  await writeProjectManifest({
    ...manifest,
    pnpm: {
      ...manifest.pnpm,
      auditConfig: {
        ...config,
      },
    },
  })
  return [...diffCve]
}

const filterAdvisoriesWithNoResolutions = (advisories: AuditAdvisory[]) => {
  // eslint-disable-next-line @typescript-eslint/naming-convention
  return advisories.filter(({ patched_versions }) => patched_versions === '<0.0.0')
}