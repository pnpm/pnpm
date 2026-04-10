import { writeSettings } from '@pnpm/config.writer'
import type { AuditAdvisory, AuditReport } from '@pnpm/deps.compliance.audit'
import { difference } from 'ramda'
import semver from 'semver'

import type { AuditOptions } from './audit.js'

export interface FixResult {
  vulnOverrides: Record<string, string>
  addedAgeExcludes: string[]
}

export async function fix (auditReport: AuditReport, opts: AuditOptions): Promise<FixResult> {
  const fixableAdvisories = getFixableAdvisories(Object.values(auditReport.advisories), opts.auditConfig?.ignoreCves, opts.auditConfig?.ignoreGhsas)
  const vulnOverrides = createOverrides(fixableAdvisories)
  if (Object.values(vulnOverrides).length === 0) return { vulnOverrides, addedAgeExcludes: [] }
  const addedAgeExcludes = opts.minimumReleaseAge ? createMinimumReleaseAgeExcludes(fixableAdvisories) : []
  await writeSettings({
    updatedOverrides: vulnOverrides,
    addedMinimumReleaseAgeExcludes: addedAgeExcludes.length > 0 ? addedAgeExcludes : undefined,
    rootProjectManifest: opts.rootProjectManifest,
    rootProjectManifestDir: opts.rootProjectManifestDir,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })
  return { vulnOverrides, addedAgeExcludes }
}

function getFixableAdvisories (advisories: AuditAdvisory[], ignoreCves?: string[], ignoreGhsas?: string[]): AuditAdvisory[] {
  if (ignoreCves) {
    advisories = advisories.filter(({ cves }) => difference(cves, ignoreCves).length > 0)
  }
  if (ignoreGhsas) {
    advisories = advisories.filter(({ github_advisory_id: ghsaId }) => difference([ghsaId], ignoreGhsas).length > 0)
  }
  return advisories
    .filter(({ vulnerable_versions: vulnerableVersions, patched_versions: patchedVersions }) => vulnerableVersions !== '>=0.0.0' && patchedVersions !== '<0.0.0')
}

function createOverrides (advisories: AuditAdvisory[]): Record<string, string> {
  return Object.fromEntries(
    advisories.map((advisory) => [
      `${advisory.module_name}@${advisory.vulnerable_versions}`,
      advisory.patched_versions,
    ])
  )
}

export function createMinimumReleaseAgeExcludes (advisories: AuditAdvisory[]): string[] {
  const excludes = new Set<string>()
  for (const advisory of advisories) {
    if (advisory.patched_versions === '<0.0.0') continue
    if (advisory.vulnerable_versions === '>=0.0.0' || advisory.vulnerable_versions === '*') continue
    const minVersion = semver.minVersion(advisory.patched_versions)
    if (minVersion) {
      excludes.add(`${advisory.module_name}@${minVersion.version}`)
    }
  }
  return Array.from(excludes)
}
