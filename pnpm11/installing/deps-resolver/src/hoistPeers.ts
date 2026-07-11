import type { PreferredVersions } from '@pnpm/resolving.resolver-base'
import { lexCompare } from '@pnpm/util.lex-comparator'
import semver from 'semver'

/** One workspace-root dependency that a missing peer can be satisfied with. */
export interface HoistableRootDep {
  alias: string
  pkgName: string
  normalizedBareSpecifier?: string
}

export function hoistPeers (
  opts: {
    autoInstallPeers: boolean
    allPreferredVersions?: PreferredVersions
    workspaceRootDeps: HoistableRootDep[]
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
      .filter((rootDep) => rootDep.pkgName === peerName)
      .sort((rootDep1, rootDep2) => lexCompare(rootDep1.alias, rootDep2.alias))[0]
    if (rootDep?.normalizedBareSpecifier) {
      dependencies[peerName] = rootDep.normalizedBareSpecifier
      continue
    }
    if (opts.allPreferredVersions![peerName]) {
      const versions: string[] = []
      const nonVersions: string[] = []
      for (const [spec, selector] of Object.entries(opts.allPreferredVersions![peerName])) {
        const specType = typeof selector === 'string' ? selector : selector.selectorType
        if (specType === 'version') {
          versions.push(spec)
        } else {
          nonVersions.push(spec)
        }
      }
      // Dedupe onto a preferred version only when it actually satisfies the
      // wanted peer range. Picking the highest preferred version regardless of
      // the range lets a version resolved for one importer be auto-installed as
      // another importer's peer even though nothing in that importer's closure
      // accepts it, silently producing a peer graph that mixes incompatible
      // majors. Ranges that are not semver (workspace:, npm: aliases, dist-tags)
      // cannot be checked, so they keep the dedupe-to-highest behavior.
      const isSemverRange = semver.validRange(range, { includePrerelease: true }) != null
      const satisfyingVersion = isSemverRange
        ? semver.maxSatisfying(versions, range, { includePrerelease: true })
        : null
      if (satisfyingVersion) {
        dependencies[peerName] = [satisfyingVersion, ...nonVersions].join(' || ')
      } else if (isSemverRange && versions.length > 0 && opts.autoInstallPeers) {
        // Preferred versions exist but none satisfies the wanted range.
        // Use the range directly so pnpm resolves it from the registry rather
        // than installing a version the peer explicitly rejects.
        dependencies[peerName] = range
      } else {
        dependencies[peerName] = [semver.maxSatisfying(versions, '*', { includePrerelease: true }), ...nonVersions]
          .filter(spec => spec != null)
          .join(' || ')
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
    for (const [version, selector] of Object.entries(allPreferredVersions[missingOptionalPeerName])) {
      const specType = typeof selector === 'string' ? selector : selector.selectorType
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
