import { mergePackageVersionSpecs } from '@pnpm/config.version-policy'
import { writeSettings } from '@pnpm/config.writer'
import { type AuditAdvisory, type AuditReport, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import semver from 'semver'

import type { AuditOptions } from './audit.js'
import { createPublishTimesFetcher, lowestNonDeprecatedVersion, type PublishTimesFetcher } from './publishTimes.js'

export interface FixResult {
  vulnOverrides: Record<string, string>
  addedAgeExcludes: string[]
}

export async function fix (auditReport: AuditReport, opts: AuditOptions): Promise<FixResult> {
  const fixableAdvisories = getFixableAdvisories(Object.values(auditReport.advisories), opts.auditConfig?.ignoreGhsas)
  const vulnOverrides = createOverrides(fixableAdvisories)
  if (Object.values(vulnOverrides).length === 0) return { vulnOverrides, addedAgeExcludes: [] }
  const addedAgeExcludes = opts.minimumReleaseAge
    ? await createMinimumReleaseAgeExcludes(fixableAdvisories, {
      getPublishTimes: opts.getPublishTimes ?? createPublishTimesFetcher(opts),
      minimumReleaseAge: opts.minimumReleaseAge,
    })
    : []
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

export interface CreateMinimumReleaseAgeExcludesOptions {
  /**
   * Publish-time lookup (the packument's `time` map plus deprecated version
   * set) per package name. `undefined` means the publish info is unknown.
   */
  getPublishTimes: PublishTimesFetcher
  /**
   * In minutes, same unit as the `minimumReleaseAge` setting.
   */
  minimumReleaseAge: number
  now?: number
}

/**
 * The `minimumReleaseAgeExclude` entries needed to keep the age gate from
 * blocking the patched versions: one entry per fixable advisory whose fix —
 * the version {@link lowestNonDeprecatedVersion} resolves the patched range
 * to — is younger than the cutoff. A version published at or before the
 * cutoff doesn't need a bypass, and a version whose publish time is unknown
 * keeps its entry so a genuinely fresh fix stays installable. An advisory the
 * packument offers no fix for gets no entry.
 */
export async function createMinimumReleaseAgeExcludes (
  advisories: AuditAdvisory[],
  opts: CreateMinimumReleaseAgeExcludesOptions
): Promise<string[]> {
  const cutoff = (opts.now ?? Date.now()) - opts.minimumReleaseAge * 60 * 1000
  const specs = await Promise.all(advisories.map(async (advisory): Promise<string | undefined> => {
    const patchedVersions = advisory.patched_versions
    if (!patchedVersions) return undefined
    const minVersion = semver.minVersion(patchedVersions)
    if (!minVersion) return undefined
    const publishInfo = await opts.getPublishTimes(advisory.module_name)
    if (publishInfo == null) return `${advisory.module_name}@${minVersion.version}`
    const lowest = lowestNonDeprecatedVersion(publishInfo, patchedVersions)
    if (lowest == null) return undefined
    const lowestSpec = `${advisory.module_name}@${lowest.version}`
    const publishTime: unknown = publishInfo.time[lowest.key]
    // The time map comes from an untrusted registry response: only a strict
    // ISO 8601 timestamp counts; anything else (including bare numbers and
    // non-ISO strings the Date constructor would accept) is treated as unknown.
    if (typeof publishTime !== 'string') return lowestSpec
    const publishedAt = parseIsoTimestamp(publishTime)
    return publishedAt == null || publishedAt > cutoff ? lowestSpec : undefined
  }))
  return mergePackageVersionSpecs(specs.filter((spec): spec is string => spec != null))
}

// RFC 3339 / ISO 8601 date-time, e.g. 2020-01-01T00:00:00.000Z.
// Rejects bare numbers and other non-ISO strings that `new Date()` would parse.
const ISO_TIMESTAMP_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/

function parseIsoTimestamp (input: string): number | undefined {
  if (!ISO_TIMESTAMP_RE.test(input)) return undefined
  const ms = new Date(input).getTime()
  return Number.isNaN(ms) ? undefined : ms
}
