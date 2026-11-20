import type { PackageMeta } from '@pnpm/resolving.registry.types'
import type { NonDeprecatedAlternative } from '@pnpm/resolving.resolver-base'
import type { PackageVersionPolicy } from '@pnpm/types'
import semver from 'semver'

import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import { semverSatisfiesLoose } from './semverLoose.js'

export interface PublishPolicyOptions {
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
}

/** Whether the policy trusts `version` outright. */
function policyTrusts (
  meta: PackageMeta,
  version: string,
  opts: PublishPolicyOptions
): boolean {
  const excludeResult = opts.publishedByExclude?.(meta.name)
  if (excludeResult === true) return true
  return Array.isArray(excludeResult) && excludeResult.includes(version)
}

/**
 * Whether the cutoff has positive evidence that `version` is too new.
 *
 * A version pnpm cannot date is not flagged, matching the resolver's own
 * violation check, which likewise only flags a version it can date.
 * Reporting wants this direction: hiding an available version over metadata
 * pnpm failed to read would be its own wrong answer.
 */
export function knownImmature (
  meta: PackageMeta,
  version: string,
  opts: PublishPolicyOptions
): boolean {
  if (!opts.publishedBy) return false
  if (policyTrusts(meta, version, opts)) return false
  const publishedAt = meta.time?.[version]
  if (publishedAt == null) return false
  const ts = new Date(publishedAt).getTime()
  return !Number.isNaN(ts) && ts > opts.publishedBy.getTime()
}

/**
 * Whether `version` clears the cutoff the way the pick's own filter requires.
 *
 * The inverse of {@link knownImmature}: admission needs positive evidence of
 * maturity, because `filterPkgMetadataByPublishDate` drops every version it
 * cannot date. Recommending a version wants this direction, so pnpm never
 * names one the pick would then refuse.
 */
export function installableUnderPolicy (
  meta: PackageMeta,
  version: string,
  opts: PublishPolicyOptions
): boolean {
  if (!opts.publishedBy) return true
  if (policyTrusts(meta, version, opts)) return true
  if (meta.time == null) {
    // Abbreviated metadata carries no per-version timestamps, and the pick
    // admits every version here once `modified` proves the whole document
    // predates the cutoff. Follow it, so a package that resolved from
    // abbreviated metadata still gets told where to go.
    const modified = parseModifiedDate(meta.modified)
    return modified != null && modified <= opts.publishedBy
  }
  const publishedAt = meta.time[version]
  if (publishedAt == null) return false
  const ts = new Date(publishedAt).getTime()
  return !Number.isNaN(ts) && ts <= opts.publishedBy.getTime()
}

/**
 * The newest version of `meta` the registry does not report as deprecated,
 * for the deprecation warning to point at.
 *
 * `meta` must already be narrowed by any active `publishedBy` policy, so the
 * version named is one pnpm would actually install. `undefined` when every
 * admissible version is deprecated. Reads deprecation off the packument pnpm
 * already holds, so it costs no extra request.
 */
export function findNonDeprecatedAlternative (
  meta: PackageMeta,
  spec: RegistryPackageSpec,
  opts: PublishPolicyOptions
): NonDeprecatedAlternative | undefined {
  const newest = findNewestInstallableNonDeprecated(meta, opts)
  if (newest == null) return undefined
  const version = newest.version
  return {
    version,
    outsideDeclaredRange: spec.type === 'range' &&
      spec.fetchSpec !== '*' &&
      !semverSatisfiesLoose(version, spec.fetchSpec),
  }
}

function findNewestInstallableNonDeprecated (
  meta: PackageMeta,
  opts: PublishPolicyOptions
): semver.SemVer | undefined {
  let newest: semver.SemVer | undefined
  // Keys first, so a lazily-loaded manifest is parsed only for a version the
  // policy admits. A version without a manifest is skipped.
  for (const version in meta.versions) {
    if (!isInstallableNonDeprecated(meta, version, opts)) continue
    const parsed = semver.parse(version, true)
    if (parsed != null && (newest == null || parsed.compare(newest) > 0)) {
      newest = parsed
    }
  }
  return newest
}

function isInstallableNonDeprecated (meta: PackageMeta, version: string, opts: PublishPolicyOptions): boolean {
  if (!Object.hasOwn(meta.versions, version) || !installableUnderPolicy(meta, version, opts)) return false
  const versionMeta = meta.versions[version]
  return versionMeta != null && !versionMeta.deprecated
}

export function parseModifiedDate (modified: string | undefined): Date | null {
  if (!modified) return null
  const date = new Date(modified)
  if (Number.isNaN(date.getTime())) return null
  return date
}
