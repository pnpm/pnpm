import { type PreferredVersions } from '@pnpm/resolver-base'
import semver from 'semver'

export function hoistPeers (
  missingRequiredPeers: Array<[string, { range: string }]>,
  opts: {
    autoInstallPeers: boolean
    allPreferredVersions?: PreferredVersions
  }
): Record<string, string> {
  const dependencies: Record<string, string> = {}
  for (const [peerName, { range }] of missingRequiredPeers) {
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

export function hoistOptionalPeers (
  allMissingOptionalPeers: Record<string, string[]>,
  allPreferredVersions: PreferredVersions
): Record<string, string> {
  const optionalDependencies: Record<string, string> = {}
  for (const [missingOptionalPeerName, ranges] of Object.entries(allMissingOptionalPeers)) {
    if (allPreferredVersions?.[missingOptionalPeerName] == null) continue
    const optionalPeerVersionThatSatisfyAll = Object.entries(allPreferredVersions[missingOptionalPeerName])
      .filter(([_, specType]) => specType === 'version')
      .find(([version]) => ranges.every((range) => semver.satisfies(version, range)))
    if (optionalPeerVersionThatSatisfyAll) {
      optionalDependencies[missingOptionalPeerName] = optionalPeerVersionThatSatisfyAll[0]
    }
  }
  return optionalDependencies
}
