import { writeSettings } from '@pnpm/config.writer'
import type { AuditAdvisory, AuditReport } from '@pnpm/deps.compliance.audit'
import semver from 'semver'

import type { AuditOptions } from './audit.js'

export interface FixResult {
  vulnOverrides: Record<string, string>
  addedAgeExcludes: string[]
}

export async function fix (auditReport: AuditReport, opts: AuditOptions): Promise<FixResult> {
  const fixableAdvisories = getFixableAdvisories(Object.values(auditReport.advisories), opts.auditConfig?.ignoreGhsas)
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

function getFixableAdvisories (advisories: AuditAdvisory[], ignoreGhsas?: string[]): AuditAdvisory[] {
  if (ignoreGhsas) {
    const ignored = new Set(ignoreGhsas)
    advisories = advisories.filter(({ github_advisory_id: ghsaId }) => !ghsaId || !ignored.has(ghsaId))
  }
  // Only advisories with a known patched range can produce an override.
  // patched_versions is undefined when pnpm couldn't infer a range from
  // vulnerable_versions; "<0.0.0" is npm's sentinel for "no fix exists".
  return advisories.filter(({ vulnerable_versions: vulnerableVersions, patched_versions: patchedVersions }) =>
    vulnerableVersions !== '>=0.0.0' && patchedVersions != null && patchedVersions !== '<0.0.0'
  )
}

function createOverrides (advisories: AuditAdvisory[]): Record<string, string> {
  const entries: Array<[string, string]> = []
  for (const advisory of advisories) {
    if (!advisory.patched_versions) continue
    entries.push([`${advisory.module_name}@${advisory.vulnerable_versions}`, advisory.patched_versions])
  }
  return Object.fromEntries(entries)
}

export function createMinimumReleaseAgeExcludes (advisories: AuditAdvisory[]): string[] {
  const excludes = new Set<string>()
  for (const advisory of advisories) {
    const patchedVersions = advisory.patched_versions
    if (!patchedVersions || patchedVersions === '<0.0.0') continue
    if (advisory.vulnerable_versions === '>=0.0.0' || advisory.vulnerable_versions === '*') continue
    const minVersion = semver.minVersion(patchedVersions)
    if (minVersion) {
      excludes.add(`${advisory.module_name}@${minVersion.version}`)
    }
  }
  return Array.from(excludes)
}
