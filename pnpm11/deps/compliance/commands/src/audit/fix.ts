import { mergePackageVersionSpecs } from '@pnpm/config.version-policy'
import { writeSettings } from '@pnpm/config.writer'
import { type AuditAdvisory, type AuditReport, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import semver from 'semver'

import type { AuditOptions } from './audit.js'
import { createPublishTimesFetcher } from './publishTimes.js'

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
   * Publish-time lookup (the packument's `time` map) per package name.
   * `undefined` means the publish times are unknown; a resolved map that
   * lacks a version means that version was never published.
   */
  getPublishTimes: (pkgName: string) => Promise<Record<string, string> | undefined>
  /**
   * In minutes, same unit as the `minimumReleaseAge` setting.
   */
  minimumReleaseAge: number
  now?: number
}

/**
 * The `minimumReleaseAgeExclude` entries needed to keep the age gate from
 * blocking the patched versions: one entry per fixable advisory whose minimum
 * published version satisfying the patched range is younger than the cutoff.
 * The theoretical range minimum may never have been published (a skipped or
 * yanked release), so the lowest published version that satisfies the range
 * is used instead — that is the version the override actually resolves to.
 * A version published at or before the cutoff doesn't need a bypass, and a
 * version whose publish time is unknown keeps its entry so a genuinely fresh
 * fix stays installable. A version absent from a successfully fetched
 * packument is not available — whether it was never published, skipped, or
 * yanked — so it gets no entry.
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
    const spec = `${advisory.module_name}@${minVersion.version}`
    const times = await opts.getPublishTimes(advisory.module_name)
    if (times == null) return spec
    // The theoretical range minimum may not exist on the registry; use the
    // lowest published version that satisfies the range, which is what the
    // override actually resolves to. The original key is retained because
    // the registry may use a non-normalized form (e.g. `v1.2.3`) that the
    // parsed SemVer drops.
    const published = Object.keys(times)
      .filter((key) => key !== 'created' && key !== 'modified')
      .map((key) => ({ key, parsed: semver.parse(key, { loose: true }) }))
      .filter((entry): entry is { key: string, parsed: semver.SemVer } => entry.parsed != null)
      .filter((entry) => semver.satisfies(entry.parsed, patchedVersions))
      .sort((a, b) => semver.compare(a.parsed, b.parsed))
    const lowest = published[0]
    if (!lowest) return undefined
    const lowestSpec = `${advisory.module_name}@${lowest.parsed.version}`
    const publishTime: unknown = times[lowest.key]
    // The time map comes from an untrusted registry response: only a string
    // can carry a real timestamp; anything else is treated as unknown.
    if (typeof publishTime !== 'string') return lowestSpec
    const publishedAt = new Date(publishTime).getTime()
    return Number.isNaN(publishedAt) || publishedAt > cutoff ? lowestSpec : undefined
  }))
  return mergePackageVersionSpecs(specs.filter((spec): spec is string => spec != null))
}
