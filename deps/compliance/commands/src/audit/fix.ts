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
  const fixableAdvisories = getFixableAdvisories(Object.values(auditReport.advisories), opts.auditConfig?.ignoreCves)
  const vulnOverrides = createOverrides(fixableAdvisories)
  if (Object.values(vulnOverrides).length === 0) return { vulnOverrides, addedAgeExcludes: [] }
  const addedAgeExcludes = createMinimumReleaseAgeExcludes(fixableAdvisories)
  await writeSettings({
    updatedOverrides: vulnOverrides,
    addedMinimumReleaseAgeExcludes: addedAgeExcludes.length > 0 ? addedAgeExcludes : undefined,
    rootProjectManifest: opts.rootProjectManifest,
    rootProjectManifestDir: opts.rootProjectManifestDir,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })
  return { vulnOverrides, addedAgeExcludes }
}

function getFixableAdvisories (advisories: AuditAdvisory[], ignoreCves?: string[]): AuditAdvisory[] {
  if (ignoreCves) {
    advisories = advisories.filter(({ cves }) => difference(cves, ignoreCves).length > 0)
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

function createMinimumReleaseAgeExcludes (advisories: AuditAdvisory[]): string[] {
  const excludes: string[] = []
  for (const advisory of advisories) {
    const minVersion = semver.minVersion(advisory.patched_versions)
    if (minVersion) {
      excludes.push(`${advisory.module_name}@${minVersion.version}`)
    }
  }
  return excludes
}
