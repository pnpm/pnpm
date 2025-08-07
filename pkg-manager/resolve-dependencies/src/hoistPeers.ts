import { type PreferredVersions } from '@pnpm/resolver-base'
import semver from 'semver'
import { type PkgAddressOrLink } from './resolveDependencies'

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
    const rootDep = opts.workspaceRootDeps.find((rootDep) => rootDep.alias === peerName)
    if (rootDep?.version) {
      dependencies[peerName] = rootDep.version
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
      dependencies[peerName] = [semver.maxSatisfying(versions, '*', { includePrerelease: true }), ...nonVersions].join(' || ')
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
