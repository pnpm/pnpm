import { type PreferredVersions } from '@pnpm/resolver-base'
import { lexCompare } from '@pnpm/util.lex-comparator'
import semver from 'semver'
import { type PkgAddressOrLink } from './resolveDependencies.js'

export function hoistPeers (
  opts: {
    autoInstallPeers: boolean
    allPreferredVersions?: PreferredVersions
    workspaceRootDeps: PkgAddressOrLink[]
  },
  missingRequiredPeers: Array<[string, { range: string }]>
): Record<string, string> {
  const dependencies: Record<string, string> = {}
  for (const [peerName, { range }] of missingRequiredPeers) {
    const rootDepByAlias = opts.workspaceRootDeps.find((rootDep) => rootDep.alias === peerName)
    if (rootDepByAlias?.normalizedBareSpecifier) {
      dependencies[peerName] = rootDepByAlias.normalizedBareSpecifier
      continue
    }
    const rootDep = opts.workspaceRootDeps
      .filter((rootDep) => rootDep.pkg.name === peerName)
      .sort((rootDep1, rootDep2) => lexCompare(rootDep1.alias, rootDep2.alias))[0]
    if (rootDep?.normalizedBareSpecifier) {
      dependencies[peerName] = rootDep.normalizedBareSpecifier
      continue
    }
    if (opts.allPreferredVersions![peerName]) {
      const versions: string[] = []
      const nonVersions: string[] = []
      for (const [spec, specType] of Object.entries(opts.allPreferredVersions![peerName])) {
        if (specType === 'version') {
          versions.push(spec)
        } else {
          nonVersions.push(spec)
        }
      }
      // When the range is an exact version (e.g. pinned by an override like "4.3.0"),
      // try to find a preferred version that satisfies it. This prevents a stale
      // higher version from the lockfile being picked over the overridden version.
      // For regular semver ranges (e.g. "^1.0.0"), use the highest preferred
      // version for deduplication.
      const isExactVersion = semver.valid(range) != null
      const satisfyingVersion = isExactVersion
        ? semver.maxSatisfying(versions, range, { includePrerelease: true })
        : null
      if (satisfyingVersion) {
        dependencies[peerName] = [satisfyingVersion, ...nonVersions].join(' || ')
      } else if (isExactVersion && opts.autoInstallPeers) {
        // No preferred version satisfies the exact override version.
        // Use the range directly so pnpm resolves it from the registry.
        dependencies[peerName] = range
      } else {
        dependencies[peerName] = [semver.maxSatisfying(versions, '*', { includePrerelease: true }), ...nonVersions].join(' || ')
      }
    } else if (opts.autoInstallPeers) {
      dependencies[peerName] = range
    }
  }
  return dependencies
}

export function getHoistableOptionalPeers (
  allMissingOptionalPeers: Record<string, string[]>,
  allPreferredVersions: PreferredVersions
): Record<string, string> {
  const optionalDependencies: Record<string, string> = {}
  for (const [missingOptionalPeerName, ranges] of Object.entries(allMissingOptionalPeers)) {
    if (!allPreferredVersions[missingOptionalPeerName]) continue

    let maxSatisfyingVersion: string | undefined
    for (const [version, specType] of Object.entries(allPreferredVersions[missingOptionalPeerName])) {
      if (
        specType === 'version' &&
        ranges.every(range => semver.satisfies(version, range)) &&
        (!maxSatisfyingVersion || semver.gt(version, maxSatisfyingVersion))
      ) {
        maxSatisfyingVersion = version
      }
    }
    if (maxSatisfyingVersion) {
      optionalDependencies[missingOptionalPeerName] = maxSatisfyingVersion
    }
  }
  return optionalDependencies
}
