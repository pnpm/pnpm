import semver from 'semver'

import { PnpmError } from '@pnpm/error'
import type { PickVersionByVersionRange, VersionSelectors, RegistryPackageSpec, PackageInRegistry, PackageMeta } from '@pnpm/types'

export function pickPackageFromMeta(
  pickVersionByVersionRangeFn: PickVersionByVersionRange | undefined,
  spec: RegistryPackageSpec,
  preferredVersionSelectors: VersionSelectors | undefined,
  meta: PackageMeta,
  publishedBy?: Date | undefined
): PackageInRegistry | null | undefined {
  if (
    (!meta.versions || Object.keys(meta.versions).length === 0) &&
    !publishedBy
  ) {
    // Unfortunately, the npm registry doesn't return the time field in the abbreviated metadata.
    // So we won't always know if the package was unpublished.
    if (meta.time?.unpublished?.versions?.length) {
      throw new PnpmError(
        'UNPUBLISHED_PKG',
        `No versions available for ${spec.name} because it was unpublished`
      )
    }

    throw new PnpmError(
      'NO_VERSIONS',
      `No versions available for ${spec.name}. The package may be unpublished.`
    )
  }

  try {
    let version: string | null | undefined

    switch (spec.type) {
      case 'version': {
        version = spec.fetchSpec

        break
      }

      case 'tag': {
        version = meta['dist-tags'][spec.fetchSpec]

        break
      }

      case 'range': {
        version = pickVersionByVersionRangeFn?.(
          meta,
          spec.fetchSpec,
          preferredVersionSelectors,
          publishedBy
        )

        break
      }
    }

    if (!version) {
      return null
    }

    const manifest = meta.versions[version]

    if (manifest && meta.name) {
      // Packages that are published to the GitHub registry are always published with a scope.
      // However, the name in the package.json for some reason may omit the scope.
      // So the package published to the GitHub registry will be published under @foo/bar
      // but the name in package.json will be just bar.
      // In order to avoid issues, we consider that the real name of the package is the one with the scope.
      manifest.name = meta.name
    }

    return manifest
  } catch (err: unknown) {
    throw new PnpmError(
      'MALFORMED_METADATA',
      `Received malformed metadata for "${spec.name}"`,
      {
        hint: 'This might mean that the package was unpublished from the registry',
      }
    )
  }
}

const semverRangeCache = new Map<string, semver.Range | null>()

// This is a performance optimization; working with string-ish semver
// causes lots of allocations and repeated work, but caching the Range
// and ensuring we give it a SemVer instance greatly speeds things up.
function semverSatisfiesLoose(version: string, range: string): boolean {
  let semverRange = semverRangeCache.get(range)

  if (typeof semverRange === 'undefined') {
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

export function pickLowestVersionByVersionRange(
  meta: PackageMeta,
  versionRange: string,
  preferredVerSels?: VersionSelectors
): string | null | undefined {
  if (preferredVerSels != null && Object.keys(preferredVerSels).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(
      meta,
      versionRange,
      preferredVerSels
    )

    for (const preferredVersions of prioritizedPreferredVersions) {
      const preferredVersion = semver.minSatisfying(
        preferredVersions,
        versionRange,
        true
      )

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

export function pickVersionByVersionRange(
  meta: PackageMeta,
  versionRange: string,
  preferredVerSels?: VersionSelectors | undefined,
  publishedBy?: Date | undefined
): string | semver.SemVer | null | undefined {
  let latest: string | undefined = meta['dist-tags'].latest

  if (preferredVerSels != null && Object.keys(preferredVerSels).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(
      meta,
      versionRange,
      preferredVerSels
    )

    for (const preferredVersions of prioritizedPreferredVersions) {
      if (
        preferredVersions.includes(latest ?? '') &&
        semverSatisfiesLoose(latest ?? '', versionRange)
      ) {
        return latest
      }

      const preferredVersion = semver.maxSatisfying(
        preferredVersions,
        versionRange,
        true
      )

      if (preferredVersion) {
        return preferredVersion
      }
    }
  }

  let versions = Object.keys(meta.versions)

  if (publishedBy) {
    versions = versions.filter(
      (version: string): boolean => {
        return new Date(meta.time?.[version] ?? '') <= publishedBy;
      }
    )
    if (!versions.includes(latest ?? '')) {
      latest = undefined
    }
  }

  if (
    latest &&
    (versionRange === '*' || semverSatisfiesLoose(latest, versionRange))
  ) {
    // Not using semver.satisfies in case of * because it does not select beta versions.
    // E.g.: 1.0.0-beta.1. See issue: https://github.com/pnpm/pnpm/issues/865
    return latest
  }

  const maxVersion = semver.maxSatisfying(versions, versionRange, true)

  // if the selected version is deprecated, try to find a non-deprecated one that satisfies the range
  if (
    maxVersion &&
    meta.versions[maxVersion]?.deprecated &&
    versions.length > 1
  ) {
    const nonDeprecatedVersions = versions
      .map((version: string): PackageInRegistry | undefined => {
        return meta.versions[version];
      })
      .filter((versionMeta: PackageInRegistry | undefined): boolean => {
        return !versionMeta?.deprecated;
      })
      .map((versionMeta: PackageInRegistry | undefined): string | undefined => {
        return versionMeta?.version;
      }).filter(Boolean)

    const maxNonDeprecatedVersion = semver.maxSatisfying(
      nonDeprecatedVersions,
      versionRange,
      true
    )

    if (maxNonDeprecatedVersion) return maxNonDeprecatedVersion
  }

  return maxVersion
}

function prioritizePreferredVersions(
  meta: PackageMeta,
  versionRange: string,
  preferredVerSelectors?: VersionSelectors
): string[][] {
  const preferredVerSelectorsArr = Object.entries(preferredVerSelectors ?? {})

  const versionsPrioritizer = new PreferredVersionsPrioritizer()

  for (const [
    preferredSelector,
    preferredSelectorType,
  ] of preferredVerSelectorsArr) {
    const { selectorType, weight } =
      typeof preferredSelectorType === 'string'
        ? { selectorType: preferredSelectorType, weight: 1 }
        : preferredSelectorType

    if (preferredSelector === versionRange) {
      continue
    }

    switch (selectorType) {
      case 'tag': {
        const selector = meta['dist-tags'][preferredSelector]

        if (typeof selector === 'string') {
          versionsPrioritizer.add(selector, weight)
        }

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

  add(version: string, weight: number) {
    if (this.preferredVersions[version]) {
      this.preferredVersions[version] += weight
    } else {
      this.preferredVersions[version] = weight
    }
  }

  versionsByPriority(): string[][] {
    const versionsByWeight = Object.entries(this.preferredVersions).reduce(
      (acc: Record<number, string[]>, [version, weight]): Record<number, string[]> => {
        acc[weight] = acc[weight] ?? []

        acc[weight]?.push(version)

        return acc
      },
      {}
    )

    return Object.keys(versionsByWeight)
      .sort((a: string, b: string): number => {
        return parseInt(b, 10) - parseInt(a, 10);
      })
      .map((weight: string): string[] | undefined => {
        return versionsByWeight[parseInt(weight, 10)];
      }).filter(Boolean)
  }
}
