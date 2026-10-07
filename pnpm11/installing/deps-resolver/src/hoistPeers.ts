import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import type { PreferredVersions, VersionSelectors } from '@pnpm/resolving.resolver-base'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import semver from 'semver'

/** One workspace-root dependency that a missing peer can be satisfied with. */
export interface HoistableRootDep {
  alias: string
  pkgName: string
  normalizedBareSpecifier?: string
}

export interface HoistPeersOptions {
  autoInstallPeers: boolean
  allPreferredVersions?: PreferredVersions
  /**
   * Whether the running update targets the peer. The lockfile's pins of a
   * targeted peer are not reused, so the peer re-resolves.
   */
  isUpdateTarget?: (peerName: string) => boolean
  /**
   * The versions of each peer that only the wanted lockfile held when peer
   * hoisting started. Taken once, before any importer hoists, so that the
   * pick does not depend on the order in which concurrent importers resolve.
   */
  lockfileOnlyVersions?: Map<string, Set<string>>
  workspaceRootDeps: HoistableRootDep[]
  /**
   * Applies `overrides` to a peer nobody declares as a dependency. Such a
   * peer has no manifest for the read-package hook to rewrite, so without
   * this it would resolve against its declared peer range and silently
   * produce the second copy the override exists to prevent.
   */
  overrideBareSpecifier?: (name: string, range: string) => string | undefined
}

export function hoistPeers (
  opts: HoistPeersOptions,
  missingRequiredPeers: Array<[string, { range: string }]>
): Record<string, string> {
  const dependencies: Record<string, string> = {}
  for (const [peerName, { range }] of missingRequiredPeers) {
    const hoistedSpec = pickHoistedPeerSpec(opts, { peerName, range })
    if (hoistedSpec != null) {
      dependencies[peerName] = hoistedSpec
    }
  }
  return dependencies
}

/**
 * The specifier to install the missing peer with, or `undefined` when the
 * peer is not hoisted.
 */
function pickHoistedPeerSpec (
  opts: HoistPeersOptions,
  { peerName, range }: { peerName: string, range: string }
): string | undefined {
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
    return overridden === '-' ? undefined : overridden
  }
  if (rootBareSpecifier) return rootBareSpecifier
  const preferredSelectors = opts.isUpdateTarget?.(peerName)
    ? omitLockfilePins(opts.allPreferredVersions![peerName])
    : opts.allPreferredVersions![peerName]
  if (!preferredSelectors) {
    return opts.autoInstallPeers ? range : undefined
  }
  return pickPreferredPeerSpec(preferredSelectors, {
    autoInstallPeers: opts.autoInstallPeers,
    lockfileOnlyVersions: opts.lockfileOnlyVersions?.get(peerName),
    range,
  })
}

function pickPreferredPeerSpec (
  preferredSelectors: VersionSelectors,
  { autoInstallPeers, lockfileOnlyVersions, range }: { autoInstallPeers: boolean, lockfileOnlyVersions?: Set<string>, range: string }
): string | undefined {
  const { versions, nonVersions } = splitVersionSelectors(preferredSelectors)
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
    ? maxSatisfyingDemotingLockfileOnly(versions, lockfileOnlyVersions, rangeForMatch)
    : null
  if (satisfyingVersion) {
    return [satisfyingVersion, ...nonVersions].join(' || ')
  }
  if (isSemverRange && versions.length > 0) {
    // Preferred versions exist but none satisfies the wanted range.
    // Use the range directly so pnpm resolves it from the registry rather
    // than installing a version the peer explicitly rejects. Without
    // autoInstallPeers, hoist nothing and leave the peer missing.
    return autoInstallPeers ? range : undefined
  }
  return [semver.maxSatisfying(versions, '*', { includePrerelease: true }), ...nonVersions]
    .filter(spec => spec != null)
    .join(' || ')
}

/**
 * The highest of `versions` satisfying `range`, or `null` when none does. A
 * version in `lockfileOnly` is picked only when no other version satisfies
 * `range`: its provider has moved on, so it must not outrank a version the
 * install resolved.
 */
function maxSatisfyingDemotingLockfileOnly (versions: string[], lockfileOnly: Set<string> | undefined, range: string): string | null {
  const resolved = lockfileOnly == null ? versions : versions.filter((version) => !lockfileOnly.has(version))
  return semver.maxSatisfying(resolved, range, { includePrerelease: true }) ??
    semver.maxSatisfying(versions, range, { includePrerelease: true })
}

/**
 * The versions of each package that only the wanted lockfile pins. Resolving a
 * version records it as a plain selector, so these are the weighted ones. A
 * `Map`, because package names such as `constructor` are also prototype keys.
 */
export function getLockfileOnlyVersions (preferredVersions: PreferredVersions): Map<string, Set<string>> {
  const lockfileOnlyVersions = new Map<string, Set<string>>()
  for (const [name, selectors] of Object.entries(preferredVersions)) {
    const versions = Object.keys(selectors).filter((version) => typeof selectors[version] !== 'string')
    if (versions.length > 0) {
      lockfileOnlyVersions.set(name, new Set(versions))
    }
  }
  return lockfileOnlyVersions
}

function splitVersionSelectors (selectors: VersionSelectors): { versions: string[], nonVersions: string[] } {
  const versions: string[] = []
  const nonVersions: string[] = []
  for (const [spec, selector] of Object.entries(selectors)) {
    if (getSelectorType(selector) === 'version') {
      versions.push(spec)
    } else {
      nonVersions.push(spec)
    }
  }
  return { versions, nonVersions }
}

function getSelectorType (selector: VersionSelectors[string]): string {
  return typeof selector === 'string' ? selector : selector.selectorType
}

/**
 * Keeps the versions resolved during this install. The lockfile's pins are the
 * weighted selectors `getPreferredVersionsFromLockfileAndManifests` seeds;
 * resolving a version records it as a plain selector.
 */
function omitLockfilePins (selectors: VersionSelectors | undefined): VersionSelectors | undefined {
  if (selectors == null) return undefined
  const resolvedSelectors = Object.fromEntries(
    Object.entries(selectors).filter(([, selector]) => typeof selector === 'string')
  ) as VersionSelectors
  return Object.keys(resolvedSelectors).length > 0 ? resolvedSelectors : undefined
}

export function getHoistableOptionalPeers (
  allMissingOptionalPeers: Record<string, string[]>,
  allPreferredVersions: PreferredVersions,
  workspaceRootDeps: HoistableRootDep[] = [],
  /**
   * Rejects a candidate that cannot be installed at the importer, such as one
   * whose own peers the importer provides at versions outside their ranges.
   */
  acceptsCandidate: (name: string, version: string) => boolean = () => true
): Record<string, string> {
  const optionalDependencies: Record<string, string> = {}
  for (const [missingOptionalPeerName, ranges] of Object.entries(allMissingOptionalPeers)) {
    if (!allPreferredVersions[missingOptionalPeerName]) continue
    const maxSatisfyingVersion = findMaxSatisfyingVersion(allPreferredVersions[missingOptionalPeerName], {
      acceptsCandidate,
      peerName: missingOptionalPeerName,
      ranges,
      rootRange: getRootRange(workspaceRootDeps, { peerName: missingOptionalPeerName, ranges }),
    })
    if (maxSatisfyingVersion) {
      optionalDependencies[missingOptionalPeerName] = maxSatisfyingVersion
    }
  }
  return optionalDependencies
}

/**
 * The workspace root's own specifier bounds the candidates the same way
 * it short-circuits `hoistPeers` above. Maximizing over every version in
 * the graph instead lets one importer's newer resolution be hoisted into
 * a sibling that declares nothing, adding a second instance of a package
 * the root already pins. A scheme specifier bounds them through the
 * version body getPeerVersionRange extracts; one with no version body
 * yields `*` and leaves them unbounded.
 */
function getRootRange (
  workspaceRootDeps: HoistableRootDep[],
  { peerName, ranges }: { peerName: string, ranges: string[] }
): string | null {
  const rootBareSpecifier = findWorkspaceRootDep(workspaceRootDeps, peerName)?.normalizedBareSpecifier
  const rootSpecifierRange = rootBareSpecifier != null ? semver.validRange(getPeerVersionRange(rootBareSpecifier)) : null
  // A disjoint specifier would bound them down to none, and the importer would then fall back to the root's own out-of-range version.
  return rootSpecifierRange != null && ranges.every(range => semver.validRange(range) != null && semver.intersects(rootSpecifierRange, range))
    ? rootSpecifierRange
    : null
}

function findMaxSatisfyingVersion (
  preferredSelectors: VersionSelectors,
  opts: {
    acceptsCandidate: (name: string, version: string) => boolean
    peerName: string
    ranges: string[]
    rootRange: string | null
  }
): string | undefined {
  let maxSatisfyingVersion: string | undefined
  for (const [version, selector] of Object.entries(preferredSelectors)) {
    if (getSelectorType(selector) !== 'version') continue
    const satisfiesAllRanges = (opts.rootRange == null || semver.satisfies(version, opts.rootRange)) &&
      opts.ranges.every(range => semver.satisfies(version, range))
    if (
      satisfiesAllRanges &&
      (!maxSatisfyingVersion || semver.gt(version, maxSatisfyingVersion)) &&
      opts.acceptsCandidate(opts.peerName, version)
    ) {
      maxSatisfyingVersion = version
    }
  }
  return maxSatisfyingVersion
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

/**
 * The specifier an optional peer picked at `version` is installed with: the
 * `workspace:` specifier of the root dependency that provides `name` when it
 * resolved to `version`, otherwise `version`.
 */
export function getOptionalPeerSpecifier (
  { name, version, workspaceRootDeps, rootDepVersions }: {
    name: string
    version: string
    workspaceRootDeps: HoistableRootDep[]
    rootDepVersions: Map<string, string>
  }
): string {
  const rootDep = findWorkspaceRootDep(workspaceRootDeps, name)
  return rootDep != null && rootDepVersions.get(rootDep.alias) === version && rootDep.normalizedBareSpecifier?.startsWith('workspace:')
    ? rootDep.normalizedBareSpecifier
    : version
}
