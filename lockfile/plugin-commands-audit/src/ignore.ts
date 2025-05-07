import { type AuditAdvisory, type AuditReport } from '@pnpm/audit'
import { type ProjectManifest, type AuditConfig } from '@pnpm/types'
import { writeSettings } from '@pnpm/config.config-writer'
import groupBy from 'ramda/src/groupBy'
import difference from 'ramda/src/difference'

export interface IgnoreVulnerabilitiesOptions {
  dir: string
  commaDelimList: string
  auditReport: AuditReport
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir: string
  auditConfig?: AuditConfig
}

export async function ignore (opts: IgnoreVulnerabilitiesOptions): Promise<string[]> {
  let ignoreAllCve = false
  const ignoreCveUserList = groupBy((s) => {
    if (s.startsWith('CVE-')) return 'CVE'
    return 'UNKNOWN'
  }, opts.commaDelimList.split(','))

  if (!ignoreCveUserList['CVE']) ignoreAllCve = true

  const currentCves = opts?.auditConfig?.ignoreCves ?? []
  const currentUniqueCves = new Set(currentCves)
  const advisoryWthNoResolutions = filterAdvisoriesWithNoResolutions(Object.values(opts.auditReport.advisories))

  if (ignoreAllCve) {
    Object.values(advisoryWthNoResolutions).forEach((advisory: AuditAdvisory) => {
      advisory.cves.forEach((cve) => currentUniqueCves.add(cve))
    })
  } else {
    ignoreCveUserList['CVE']?.forEach((cve) => currentUniqueCves.add(cve))
  }

  const newIgnoreCves = currentUniqueCves.size > 0 ? Array.from(currentUniqueCves) : undefined
  const diffCve = difference(newIgnoreCves ?? [], currentCves)
  await writeSettings({
    ...opts,
    updatedSettings: {
      auditConfig: {
        ...opts.auditConfig,
        ignoreCves: newIgnoreCves,
      },
    },
  })
  return [...diffCve]
}

function filterAdvisoriesWithNoResolutions (advisories: AuditAdvisory[]) {
  // eslint-disable-next-line @typescript-eslint/naming-convention
  return advisories.filter(({ patched_versions }) => patched_versions === '<0.0.0')
}
