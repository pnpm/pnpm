import { mergePackageVersionSpecs } from '@pnpm/config.version-policy'
import { writeSettings } from '@pnpm/config.writer'
import { type AuditAdvisory, type AuditReport, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import pLimit from 'p-limit'
import semver from 'semver'

import type { AuditOptions } from './audit.js'
import { createGetPublishedVersions, type GetPublishedVersions } from './publishedVersions.js'

export interface FixResult {
  vulnOverrides: Record<string, string>
  addedAgeExcludes: string[]
}

export async function fix (auditReport: AuditReport, opts: AuditOptions): Promise<FixResult> {
  const fixableAdvisories = getFixableAdvisories(Object.values(auditReport.advisories), opts.auditConfig?.ignoreGhsas)
  const resolvableAdvisories = await filterResolvableAdvisories(fixableAdvisories, createGetPublishedVersions(opts))
  const vulnOverrides = createOverrides(resolvableAdvisories)
  if (Object.values(vulnOverrides).length === 0) return { vulnOverrides, addedAgeExcludes: [] }
  const addedAgeExcludes = opts.minimumReleaseAge ? createMinimumReleaseAgeExcludes(resolvableAdvisories) : []
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

const PACKUMENT_FETCH_CONCURRENCY = 10

/**
 * `patched_versions` is inferred from the advisory's vulnerable range, so its
 * lower bound is a guess: for a `<=X.Y.Z` range it is `X.Y.(Z+1)`, which is
 * only a real release if the maintainers patched that branch. When they didn't
 * (the fix landed in a later major, or there is no fix at all), the override
 * built from it can never be satisfied and every subsequent install fails with
 * ERR_PNPM_NO_MATCHING_VERSION. Drop those advisories instead: reporting a
 * vulnerability as unfixed is better than writing an override that breaks the
 * project. Advisories are kept when the registry can't tell us what is
 * published, so an offline or private registry doesn't silently disable fixes.
 */
export async function filterResolvableAdvisories (
  advisories: AuditAdvisory[],
  getPublishedVersions: GetPublishedVersions
): Promise<AuditAdvisory[]> {
  const limit = pLimit(PACKUMENT_FETCH_CONCURRENCY)
  const resolvable = await Promise.all(advisories.map((advisory) => limit(async () => {
    if (!advisory.patched_versions) return false
    const publishedVersions = await getPublishedVersions(advisory.module_name)
    if (publishedVersions == null) return true
    const range = caretRangeForPatched(advisory.patched_versions)
    return publishedVersions.some((version) => satisfiesSafe(version, range))
  })))
  return advisories.filter((_, index) => resolvable[index])
}

function satisfiesSafe (version: string, range: string): boolean {
  try {
    return semver.satisfies(version, range, { includePrerelease: true, loose: true })
  } catch {
    return false
  }
}

function createOverrides (advisories: AuditAdvisory[]): Record<string, string> {
  const entries: Array<[string, string]> = []
  for (const advisory of advisories) {
    if (!advisory.patched_versions) continue
    entries.push([`${advisory.module_name}@${advisory.vulnerable_versions}`, caretRangeForPatched(advisory.patched_versions)])
  }
  return sortDirectKeys(Object.fromEntries(entries))
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
  const specs: string[] = []
  for (const advisory of advisories) {
    const patchedVersions = advisory.patched_versions
    if (!patchedVersions) continue
    const minVersion = semver.minVersion(patchedVersions)
    if (!minVersion) continue
    specs.push(`${advisory.module_name}@${minVersion.version}`)
  }
  return mergePackageVersionSpecs(specs)
}
