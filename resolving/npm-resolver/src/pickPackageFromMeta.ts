import { PnpmError } from '@pnpm/error'
import { type VersionSelectors } from '@pnpm/resolver-base'
import semver from 'semver'
import { type RegistryPackageSpec } from './parsePref'
import { type PackageInRegistry, type PackageMeta } from './pickPackage'

export type PickVersionByVersionRange = (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSels?: VersionSelectors,
  publishedBy?: Date
) => string | null

export function pickPackageFromMeta (
  pickVersionByVersionRangeFn: PickVersionByVersionRange,
  spec: RegistryPackageSpec,
  preferredVersionSelectors: VersionSelectors | undefined,
  meta: PackageMeta,
  publishedBy?: Date
): PackageInRegistry | null {
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
      version = pickVersionByVersionRangeFn(meta, spec.fetchSpec, preferredVersionSelectors, publishedBy)
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
  } catch (err: any) { // eslint-disable-line
    throw new PnpmError('MALFORMED_METADATA',
      `Received malformed metadata for "${spec.name}"`,
      { hint: 'This might mean that the package was unpublished from the registry' }
    )
  }
}

export function pickLowestVersionByVersionRange (
  meta: PackageMeta,
  versionRange: string
) {
  if (versionRange === '*') {
    return Object.keys(meta.versions).sort(semver.compare)[0]
  }
  return semver.minSatisfying(Object.keys(meta.versions), versionRange, true)
}

export function pickVersionByVersionRange (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSels?: VersionSelectors,
  publishedBy?: Date
) {
  let latest: string | undefined = meta['dist-tags'].latest

  if (preferredVerSels != null && Object.keys(preferredVerSels).length > 0) {
    const prioritizedPreferredVersions = prioritizePreferredVersions(meta, versionRange, preferredVerSels)
    for (const preferredVersions of prioritizedPreferredVersions) {
      if (preferredVersions.includes(latest) && semver.satisfies(latest, versionRange, true)) {
        return latest
      }
      const preferredVersion = semver.maxSatisfying(preferredVersions, versionRange, true)
      if (preferredVersion) {
        return preferredVersion
      }
    }
  }

  let versions = Object.keys(meta.versions)
  if (publishedBy) {
    versions = versions.filter(version => new Date(meta.time![version]) <= publishedBy)
    if (!versions.includes(latest)) {
      latest = undefined
    }
  }
  if (latest && (versionRange === '*' || semver.satisfies(latest, versionRange, true))) {
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
  preferredVerSels?: VersionSelectors
): string[][] {
  const preferredVerSelsArr = Object.entries(preferredVerSels ?? {})
  const versionsPrioritizer = new PreferredVersionsPrioritizer()
  for (const [preferredSelector, preferredSelectorType] of preferredVerSelsArr) {
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
      // This might be slow if there are many versions
      // and the package is an indirect dependency many times in the project.
      // If it will create noticeable slowdown, then might be a good idea to add some caching
      const versions = Object.keys(meta.versions)
      for (const version of versions) {
        if (semver.satisfies(version, preferredSelector, true)) {
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

  add (version: string, weight: number) {
    if (!this.preferredVersions[version]) {
      this.preferredVersions[version] = weight
    } else {
      this.preferredVersions[version] += weight
    }
  }

  versionsByPriority () {
    const versionsByWeight = Object.entries(this.preferredVersions)
      .reduce((acc, [version, weight]) => {
        acc[weight] = acc[weight] ?? []
        acc[weight].push(version)
        return acc
      }, {} as Record<number, string[]>)
    return Object.keys(versionsByWeight)
      .sort((a, b) => parseInt(b, 10) - parseInt(a, 10))
      .map((weigth) => versionsByWeight[parseInt(weigth, 10)])
  }
}
