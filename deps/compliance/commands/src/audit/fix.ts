import { writeSettings } from '@pnpm/config.writer'
import { type AuditAdvisory, type AuditReport, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
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
    // Normalize on both sides so ignore entries match regardless of casing.
    const ignored = new Set(ignoreGhsas.map(normalizeGhsaId))
    advisories = advisories.filter(({ github_advisory_id: ghsaId }) => !ghsaId || !ignored.has(normalizeGhsaId(ghsaId)))
  }
  // Only advisories with a known patched range can produce an override.
  // patched_versions is undefined when the range couldn't be inferred from
  // vulnerable_versions — no override is possible in that case.
  return advisories.filter(({ patched_versions: patchedVersions }) => patchedVersions != null)
}

function createOverrides (advisories: AuditAdvisory[]): Record<string, string> {
  const entries: Array<[string, string]> = []
  for (const advisory of advisories) {
    if (!advisory.patched_versions) continue
    entries.push([`${advisory.module_name}@${advisory.vulnerable_versions}`, caretRangeForPatched(advisory.patched_versions)])
  }
  return Object.fromEntries(entries)
}

// Use the minimum patched version with a caret so pnpm stays within the
// same major as the fix. `>=X.Y.Z` alone can silently promote a dep to a
// later breaking major; `^X.Y.Z` still satisfies the patch while
// preserving the major the user originally pinned to.
export function caretRangeForPatched (patchedRange: string): string {
  const min = semver.minVersion(patchedRange)
  return min ? `^${min.version}` : patchedRange
}

export function createMinimumReleaseAgeExcludes (advisories: AuditAdvisory[]): string[] {
  const excludes = new Set<string>()
  for (const advisory of advisories) {
    const patchedVersions = advisory.patched_versions
    if (!patchedVersions) continue
    const minVersion = semver.minVersion(patchedVersions)
    if (minVersion) {
      excludes.add(`${advisory.module_name}@${minVersion.version}`)
    }
  }
  return Array.from(excludes)
}
