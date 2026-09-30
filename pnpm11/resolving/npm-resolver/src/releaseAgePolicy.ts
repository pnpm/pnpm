import type { PackageMeta } from '@pnpm/resolving.registry.types'
import type { Resolution, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy } from '@pnpm/types'

import { knownImmature } from './pickPackageFromMeta.js'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE } from './violationCodes.js'

/**
 * The raw `dist-tags.latest` when the active `minimumReleaseAge` policy would
 * allow installing it, `undefined` otherwise. The install summary's
 * "(X is available)" hint must only ever name the actual latest tag, so an
 * immature latest suppresses the hint instead of being rewritten to an older
 * mature version. Suppression requires positive evidence of immaturity: a
 * missing or unparsable timestamp keeps the raw tag, matching
 * `detectMinReleaseAgeViolation`, which likewise only flags a version it can
 * date.
 */
export function latestAllowedByPolicy (
  meta: PackageMeta,
  opts: {
    publishedBy?: Date
    publishedByExclude?: PackageVersionPolicy
  }
): string | undefined {
  const latest = meta['dist-tags'].latest
  if (!latest) return undefined
  return knownImmature(meta, latest, opts) ? undefined : latest
}

/**
 * Inline minimumReleaseAge detection: returns a violation entry when the
 * picked version's publish timestamp is past the policy cutoff (and
 * isn't covered by `publishedByExclude`). The resolver already has the
 * timestamp in hand, so reporting inline saves the install layer from
 * re-walking the resolved tree and re-fetching the same metadata. The
 * deps-resolver aggregates the per-resolve `policyViolation` fields into
 * a single set the install command reacts to.
 *
 * Returns `undefined` for resolutions outside the policy — no policy
 * active, version excluded by pattern, timestamp missing or malformed,
 * or version mature. Specific-version exclusions (`pkg@1.0.0`) and
 * full-name exclusions (`pkg`) are both honored so an entry already on
 * the user's exclude list isn't re-announced every install.
 */
export function detectMinReleaseAgeViolation (args: {
  name: string
  version: string
  publishedAt: string | undefined
  resolution: Resolution
  publishedBy: Date | undefined
  publishedByExclude: PackageVersionPolicy | undefined
}): ResolutionPolicyViolation | undefined {
  if (!args.publishedBy || !args.publishedAt) return undefined
  const excludeResult = args.publishedByExclude?.(args.name)
  if (excludeResult === true) return undefined
  if (Array.isArray(excludeResult) && excludeResult.includes(args.version)) return undefined
  const ts = new Date(args.publishedAt).getTime()
  if (Number.isNaN(ts) || ts <= args.publishedBy.getTime()) return undefined
  return {
    name: args.name,
    version: args.version,
    resolution: args.resolution,
    code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
    reason: `was published at ${new Date(ts).toISOString()}, within the minimumReleaseAge cutoff (${args.publishedBy.toISOString()})`,
  }
}
