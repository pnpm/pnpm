import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import type { PreferredVersions } from '@pnpm/resolving.resolver-base'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
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
    /**
     * Applies `overrides` to a peer nobody declares as a dependency. Such a
     * peer has no manifest for the read-package hook to rewrite, so without
     * this it would resolve against its declared peer range and silently
     * produce the second copy the override exists to prevent.
     */
    overrideBareSpecifier?: (name: string, range: string) => string | undefined
  },
  missingRequiredPeers: Array<[string, { range: string }]>
): Record<string, string> {
  const dependencies: Record<string, string> = {}
  for (const [peerName, { range }] of missingRequiredPeers) {
    const rootBareSpecifier = findWorkspaceRootDep(opts.workspaceRootDeps, peerName)?.normalizedBareSpecifier
    // An override redirects a hoist; it must never create one, or disabling
    // autoInstallPeers would still install a peer nobody depends on. Only the
    // workspace root's own dependency hoists a peer that autoInstallPeers is
    // not asking for, so that is the one hoist an override still governs here;
    // the deduplication below installs nothing new either way.
    const overridden = opts.autoInstallPeers || rootBareSpecifier
      ? opts.overrideBareSpecifier?.(peerName, range)
      : undefined
    if (overridden != null) {
      if (overridden !== '-') {
        dependencies[peerName] = overridden
      }
      continue
    }
    if (rootBareSpecifier) {
      dependencies[peerName] = rootBareSpecifier
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
      // majors. Scheme specifiers (named-registry, npm: aliases, workspace:)
      // contribute a comparable range through getPeerVersionRange, so they get
      // range-aware selection too; specs with no version body (catalog:,
      // dist-tags) yield a non-semver value and keep the dedupe-to-highest
      // behavior. The raw scheme is preserved below so the fallback still
      // selects the package to install.
      const rangeForMatch = getPeerVersionRange(range)
      const isSemverRange = semver.validRange(rangeForMatch, { includePrerelease: true }) != null
      const satisfyingVersion = isSemverRange
        ? semver.maxSatisfying(versions, rangeForMatch, { includePrerelease: true })
        : null
      if (satisfyingVersion) {
        dependencies[peerName] = [satisfyingVersion, ...nonVersions].join(' || ')
      } else if (isSemverRange && versions.length > 0) {
        // Preferred versions exist but none satisfies the wanted range.
        // Use the range directly so pnpm resolves it from the registry rather
        // than installing a version the peer explicitly rejects. Without
        // autoInstallPeers, hoist nothing and leave the peer missing.
        if (opts.autoInstallPeers) {
          dependencies[peerName] = range
        }
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
  allPreferredVersions: PreferredVersions,
  workspaceRootDeps: HoistableRootDep[] = []
): Record<string, string> {
  const optionalDependencies: Record<string, string> = {}
  for (const [missingOptionalPeerName, ranges] of Object.entries(allMissingOptionalPeers)) {
    if (!allPreferredVersions[missingOptionalPeerName]) continue

    // The workspace root's own specifier bounds the candidates the same way
    // it short-circuits `hoistPeers` above. Maximizing over every version in
    // the graph instead lets one importer's newer resolution be hoisted into
    // a sibling that declares nothing, adding a second instance of a package
    // the root already pins. A scheme specifier bounds them through the
    // version body getPeerVersionRange extracts; one with no version body
    // yields `*` and leaves them unbounded.
    const rootBareSpecifier = findWorkspaceRootDep(workspaceRootDeps, missingOptionalPeerName)?.normalizedBareSpecifier
    const rootSpecifierRange = rootBareSpecifier != null ? semver.validRange(getPeerVersionRange(rootBareSpecifier)) : null
    // A disjoint specifier would bound them down to none, and the importer would then fall back to the root's own out-of-range version.
    const rootRange = rootSpecifierRange != null && ranges.every(range => semver.validRange(range) != null && semver.intersects(rootSpecifierRange, range))
      ? rootSpecifierRange
      : null

    let maxSatisfyingVersion: string | undefined
    for (const [version, selector] of Object.entries(allPreferredVersions[missingOptionalPeerName])) {
      const specType = typeof selector === 'string' ? selector : selector.selectorType
      if (
        specType === 'version' &&
        (rootRange == null || semver.satisfies(version, rootRange)) &&
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

/**
 * The root dependency that provides `peerName`: an alias match wins over a
 * package-name match (an `npm:` alias can install the same package under a
 * different slot), and among package-name matches the lexicographically
 * first alias wins so the pick is stable. Only a dependency that has a
 * normalized specifier is a candidate — the callers have nothing to install
 * or bound the peer with otherwise.
 */
function findWorkspaceRootDep (
  workspaceRootDeps: HoistableRootDep[],
  peerName: string
): HoistableRootDep | undefined {
  // One allocation-free pass: this runs for every missing peer of every
  // importer on each hoist round.
  let rootDepByPkgName: HoistableRootDep | undefined
  for (const rootDep of workspaceRootDeps) {
    if (!rootDep.normalizedBareSpecifier) continue
    if (rootDep.alias === peerName) return rootDep
    if (
      rootDep.pkgName === peerName &&
      (rootDepByPkgName == null || lexCompare(rootDep.alias, rootDepByPkgName.alias) < 0)
    ) {
      rootDepByPkgName = rootDep
    }
  }
  return rootDepByPkgName
}
