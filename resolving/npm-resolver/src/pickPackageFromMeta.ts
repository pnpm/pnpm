import { PnpmError } from '@pnpm/error'
import { filterPkgMetadataByPublishDate } from '@pnpm/registry.pkg-metadata-filter'
import { type PackageInRegistry, type PackageMeta, type PackageMetaWithTime } from '@pnpm/registry.types'
import { type VersionSelectors } from '@pnpm/resolver-base'
import { type PackageVersionPolicy } from '@pnpm/types'
import semver from 'semver'
import util from 'util'
import { type RegistryPackageSpec } from './parseBareSpecifier.js'

export interface PickVersionByVersionRangeOptions {
  meta: PackageMeta
  versionRange: string
  preferredVersionSelectors?: VersionSelectors
  publishedBy?: Date
}

export type PickVersionByVersionRange = (options: PickVersionByVersionRangeOptions) => string | null

export interface PickPackageFromMetaOptions {
  preferredVersionSelectors: VersionSelectors | undefined
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
}

export function pickPackageFromMeta (
  pickVersionByVersionRangeFn: PickVersionByVersionRange,
  {
    preferredVersionSelectors,
    publishedBy,
    publishedByExclude,
  }: PickPackageFromMetaOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  if (publishedBy) {
    const excludeResult = publishedByExclude?.(meta.name) ?? false
    if (excludeResult !== true) {
      assertMetaHasTime(meta)
      const trustedVersions = Array.isArray(excludeResult) ? excludeResult : undefined
      meta = filterPkgMetadataByPublishDate(meta, publishedBy, trustedVersions)
    }
  }
  if ((!meta.versions || Object.keys(meta.versions).length === 0) && !publishedBy) {
    // Unfortunately, the npm registry doesn't return the time field in the abbreviated metadata.
    // So we won't always know if the package was unpublished.
    if (meta.time?.unpublished?.versions?.length) {
      throw new PnpmError('UNPUBLISHED_PKG', `No versions available for ${spec.name} because it was unpublished`)
    }
    throw new PnpmError('NO_VERSIONS', `No versions available for ${spec.name}. The package may be unpublished.`)
  }
  try {
    let version!: string | null
    switch (spec.type) {
    case 'version':
      version = spec.fetchSpec
      break
    case 'tag':
      version = meta['dist-tags'][spec.fetchSpec]
      break
    case 'range':
      version = pickVersionByVersionRangeFn({
        meta,
        versionRange: spec.fetchSpec,
        preferredVersionSelectors,
        publishedBy,
      })
      break
    }
    if (!version) return null
    const manifest = meta.versions[version]
    if (manifest && meta['name']) {
      // Packages that are published to the GitHub registry are always published with a scope.
      // However, the name in the package.json for some reason may omit the scope.
      // So the package published to the GitHub registry will be published under @foo/bar
      // but the name in package.json will be just bar.
      // In order to avoid issues, we consider that the real name of the package is the one with the scope.
      manifest.name = meta['name']
    }
    return manifest
  } catch (err: unknown) {
    if (
      util.types.isNativeError(err) &&
      'code' in err &&
      typeof err.code === 'string' &&
      err.code.startsWith('ERR_PNPM_')
    ) {
      throw err
    }
    throw new PnpmError('MALFORMED_METADATA',
      `Received malformed metadata for "${spec.name}"`,
      { hint: 'This might mean that the package was unpublished from the registry', cause: err }
    )
  }
}

export function assertMetaHasTime (meta: PackageMeta): asserts meta is PackageMetaWithTime {
  if (meta.time == null) {
    throw new PnpmError('MISSING_TIME', `The metadata of ${meta.name} is missing the "time" field`)
  }
}

const semverRangeCache = new Map<string, semver.Range | null>()

// This is a performance optimization; working with string-ish semver
// causes lots of allocations and repeated work, but caching the Range
// and ensuring we give it a SemVer instance greatly speeds things up.
function semverSatisfiesLoose (version: string, range: string): boolean {
  let semverRange = semverRangeCache.get(range)
  if (semverRange === undefined) {
    try {
      semverRange = new semver.Range(range, true)
    } catch {
      semverRange = null
    }
    semverRangeCache.set(range, semverRange)
  }

  if (semverRange) {
    try {
      return semverRange.test(new semver.SemVer(version, true))
    } catch {
      return false
    }
  }

  return false
}

export function pickLowestVersionByVersionRange (
  { meta, versionRange, preferredVersionSelectors }: PickVersionByVersionRangeOptions
): string | null {
  if (preferredVersionSelectors != null && Object.keys(preferredVersionSelectors).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVersionSelectors)
    for (const preferredVersions of prioritizedPreferredVersions) {
      const preferredVersion = semver.minSatisfying(preferredVersions, versionRange, true)
      if (preferredVersion) {
        return preferredVersion
      }
    }
  }
  if (versionRange === '*') {
    return Object.keys(meta.versions).sort(semver.compare)[0]
  }
  return semver.minSatisfying(Object.keys(meta.versions), versionRange, true)
}

export function pickVersionByVersionRange ({ meta, versionRange, preferredVersionSelectors }: PickVersionByVersionRangeOptions): string | null {
  const latest: string | undefined = meta['dist-tags'].latest

  if (preferredVersionSelectors != null && Object.keys(preferredVersionSelectors).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVersionSelectors)
    for (const preferredVersions of prioritizedPreferredVersions) {
      if (preferredVersions.includes(latest) && semverSatisfiesLoose(latest, versionRange)) {
        return latest
      }
      const preferredVersion = semver.maxSatisfying(preferredVersions, versionRange, true)
      if (preferredVersion) {
        return preferredVersion
      }
    }
  }

  const versions = Object.keys(meta.versions)
  if (latest && (versionRange === '*' || semverSatisfiesLoose(latest, versionRange))) {
    // Not using semver.satisfies in case of * because it does not select beta versions.
    // E.g.: 1.0.0-beta.1. See issue: https://github.com/pnpm/pnpm/issues/865
    return latest
  }

  const maxVersion = semver.maxSatisfying(versions, versionRange, true)

  // if the selected version is deprecated, try to find a non-deprecated one that satisfies the range
  if (maxVersion && meta.versions[maxVersion].deprecated && versions.length > 1) {
    const nonDeprecatedVersions = versions.map((version) => meta.versions[version])
      .filter((versionMeta) => !versionMeta.deprecated)
      .map((versionMeta) => versionMeta.version)

    const maxNonDeprecatedVersion = semver.maxSatisfying(nonDeprecatedVersions, versionRange, true)
    if (maxNonDeprecatedVersion) return maxNonDeprecatedVersion
  }
  return maxVersion
}

function prioritizePreferredVersions (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSelectors?: VersionSelectors
): string[][] {
  const preferredVerSelectorsArr = Object.entries(preferredVerSelectors ?? {})
  const versionsPrioritizer = new PreferredVersionsPrioritizer()
  for (const [preferredSelector, preferredSelectorType] of preferredVerSelectorsArr) {
    const { selectorType, weight } = typeof preferredSelectorType === 'string'
      ? { selectorType: preferredSelectorType, weight: 1 }
      : preferredSelectorType
    if (preferredSelector === versionRange) continue
    switch (selectorType) {
    case 'tag': {
      versionsPrioritizer.add(meta['dist-tags'][preferredSelector], weight)
      break
    }
    case 'range': {
      const versions = Object.keys(meta.versions)
      for (const version of versions) {
        if (semverSatisfiesLoose(version, preferredSelector)) {
          versionsPrioritizer.add(version, weight)
        }
      }
      break
    }
    case 'version': {
      if (meta.versions[preferredSelector]) {
        versionsPrioritizer.add(preferredSelector, weight)
      }
      break
    }
    }
  }
  return versionsPrioritizer.versionsByPriority()
}

class PreferredVersionsPrioritizer {
  private preferredVersions: Record<string, number> = {}

  add (version: string, weight: number): void {
    if (!this.preferredVersions[version]) {
      this.preferredVersions[version] = weight
    } else {
      this.preferredVersions[version] += weight
    }
  }

  versionsByPriority (): string[][] {
    const versionsByWeight = Object.entries(this.preferredVersions)
      .reduce((acc, [version, weight]) => {
        acc[weight] = acc[weight] ?? []
        acc[weight].push(version)
        return acc
      }, {} as Record<number, string[]>)
    return Object.keys(versionsByWeight)
      .sort((a, b) => parseInt(b, 10) - parseInt(a, 10))
      .map((weight) => versionsByWeight[parseInt(weight, 10)])
  }
}
