import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import type { PreferredVersions, VersionSelectors } from '@pnpm/resolving.resolver-base'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { PackageManifest } from '@pnpm/types'
import * as semverUtils from '@yarnpkg/core/semverUtils'
import semver from 'semver'

/** One workspace-root dependency that a missing peer can be satisfied with. */
export interface HoistableRootDep {
  alias: string
  pkgName: string
  normalizedBareSpecifier?: string
}

export type ProviderPick<Provider> =
  | { kind: 'bound', provider: Provider, pinnedByLockfile: boolean }
  | { kind: 'missing' }

/** What a node's scope offers for one of its declared peers. */
export interface PeerScope<Provider extends { version: string }> {
  /** The provider the nearest ancestor binds the name to. */
  inherited?: Provider
  /** Whether `inherited` wins over a locked provider. Only asked when a locked provider would otherwise win. */
  inheritedMustWin: () => boolean
  /** The provider the wanted lockfile recorded, resolved to the same path in this pass. Built only when asked. */
  locked?: () => Provider
}

/**
 * Binds the peer `peerRange` of a node to a provider in `scope`. The locked
 * provider wins when the inherited one need not win and it satisfies the
 * range. Otherwise the inherited provider binds.
 *
 * Two decisions that shape a peer suffix are not provider picks: the cycle
 * cut, and the variant `dedupePeerDependents` keeps.
 */
export function providerFor<Provider extends { version: string }> (
  peerRange: string,
  scope: PeerScope<Provider>
): ProviderPick<Provider> {
  if (scope.locked != null && !scope.inheritedMustWin()) {
    const locked = scope.locked()
    if (semverUtils.satisfiesWithPrereleases(locked.version, getPeerVersionRange(peerRange), true)) {
      return { kind: 'bound', provider: locked, pinnedByLockfile: true }
    }
  }
  return scope.inherited == null
    ? { kind: 'missing' }
    : { kind: 'bound', provider: scope.inherited, pinnedByLockfile: false }
}

/**
 * The peers of `pkg` that shadow its own `dependencies` entry of the same
 * name, which the peer edge therefore supplies. Under `autoInstallPeers`
 * every required peer shadows, since it is auto-installed. Otherwise, and
 * for optional peers, only the peers `inScope` shadow.
 */
export function getPeerShadowedDependencies (
  pkg: Pick<PackageManifest, 'dependencies' | 'peerDependencies' | 'peerDependenciesMeta'>,
  opts: { autoInstallPeers: boolean, inScope: (alias: string) => boolean }
): string[] {
  const { dependencies, peerDependencies, peerDependenciesMeta } = pkg
  if (!peerDependencies || !dependencies) return []
  return Object.keys(peerDependencies).filter((peerDep) =>
    (opts.autoInstallPeers && peerDependenciesMeta?.[peerDep]?.optional !== true) || opts.inScope(peerDep)
  )
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
  const nonVersions = getNonVersionSelectors(preferredSelectors)
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
  if (isSemverRange) {
    const satisfyingVersion = pickProviderVersion(preferredSelectors, {
      tiers: getRequiredPeerTiers(preferredSelectors, lockfileOnlyVersions),
      qualifies: (version) => semver.satisfies(version, rangeForMatch, { includePrerelease: true }),
    })
    if (satisfyingVersion) {
      return [satisfyingVersion, ...nonVersions].join(' || ')
    }
    if (hasVersionSelector(preferredSelectors)) {
      // Preferred versions exist but none satisfies the wanted range.
      // Use the range directly so pnpm resolves it from the registry rather
      // than installing a version the peer explicitly rejects. Without
      // autoInstallPeers, hoist nothing and leave the peer missing.
      return autoInstallPeers ? range : undefined
    }
  }
  return [pickProviderVersion(preferredSelectors, { qualifies: () => true }), ...nonVersions]
    .filter(spec => spec != null)
    .join(' || ')
}

/**
 * The `locked` versions this run resolved, then every version it resolved,
 * then the `locked` ones. A locked version the run no longer resolves
 * survives only as a lockfile weight, since whatever provided it has moved
 * on, so it must not outrank what the graph now holds. A version only the
 * lockfile holds loses to the resolved ones even when nothing is locked.
 */
function getOptionalPeerTiers (
  selectors: VersionSelectors,
  { locked = new Set(), lockfileOnly }: { locked?: Set<string>, lockfileOnly?: Set<string> }
): Array<Set<string>> {
  const resolved = new Set(Object.keys(selectors).filter((version) => !lockfileOnly?.has(version)))
  const reached = new Set([...locked].filter((version) => resolved.has(version)))
  return [reached, resolved, locked].filter((tier) => tier.size > 0)
}

/**
 * The versions this install resolved, so a version in `lockfileOnly` is
 * picked only when no other version qualifies. Its provider has moved on, so
 * it must not outrank a version the install resolved.
 */
function getRequiredPeerTiers (selectors: VersionSelectors, lockfileOnly: Set<string> | undefined): Array<Set<string>> {
  if (lockfileOnly == null) return []
  return [new Set(Object.keys(selectors).filter((version) => !lockfileOnly.has(version)))]
}

/**
 * The highest version among `selectors` that `qualifies`, from the first of
 * `tiers` that holds one. When no tier does, every version competes. On equal
 * versions the first selector wins.
 */
function pickProviderVersion (
  selectors: VersionSelectors,
  { tiers = [], qualifies }: { tiers?: Array<Set<string>>, qualifies: (version: string) => boolean }
): string | undefined {
  for (const tier of tiers) {
    const picked = pickHighestVersion(selectors, { tier, qualifies })
    if (picked != null) return picked
  }
  return pickHighestVersion(selectors, { qualifies })
}

/**
 * `qualifies` runs only for a version that would become the new highest, so
 * an expensive check such as `acceptsCandidate` runs as few times as possible.
 */
function pickHighestVersion (
  selectors: VersionSelectors,
  { tier, qualifies }: { tier?: Set<string>, qualifies: (version: string) => boolean }
): string | undefined {
  let highest: string | undefined
  for (const [version, selector] of Object.entries(selectors)) {
    if (getSelectorType(selector) !== 'version' || (tier != null && !tier.has(version)) || semver.valid(version) == null) continue
    if ((highest == null || semver.gt(version, highest)) && qualifies(version)) {
      highest = version
    }
  }
  return highest
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

function getNonVersionSelectors (selectors: VersionSelectors): string[] {
  return Object.keys(selectors).filter((spec) => getSelectorType(selectors[spec]) !== 'version')
}

function hasVersionSelector (selectors: VersionSelectors): boolean {
  return Object.values(selectors).some((selector) => getSelectorType(selector) === 'version')
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

/** The wanted lockfile's view of the optional peers an importer may hoist. */
export interface OptionalPeerLockfileVersions {
  /** The peer versions the wanted lockfile pinned for the importer. See `getImporterLockedPeerVersions`. */
  lockedVersions?: Map<string, Set<string>>
  /** See `HoistPeersOptions.lockfileOnlyVersions`. */
  lockfileOnlyVersions?: Map<string, Set<string>>
}

export function getHoistableOptionalPeers (
  allMissingOptionalPeers: Record<string, string[]>,
  allPreferredVersions: PreferredVersions,
  workspaceRootDeps: HoistableRootDep[] = [],
  /**
   * Rejects a candidate that cannot be installed at the importer, such as one
   * whose own peers the importer provides at versions outside their ranges.
   */
  acceptsCandidate: (name: string, version: string) => boolean = () => true,
  lockfile: OptionalPeerLockfileVersions = {}
): Record<string, string> {
  const optionalDependencies: Record<string, string> = {}
  for (const [missingOptionalPeerName, ranges] of Object.entries(allMissingOptionalPeers)) {
    if (!allPreferredVersions[missingOptionalPeerName]) continue
    const rootRange = getRootRange(workspaceRootDeps, { peerName: missingOptionalPeerName, ranges })
    // Strict, unlike the required-peer pick: an optional peer nobody declared
    // is installed only to deduplicate, so a prerelease its range rejects is
    // not worth splitting a package family over.
    const selectors = allPreferredVersions[missingOptionalPeerName]
    const maxSatisfyingVersion = pickProviderVersion(selectors, {
      tiers: getOptionalPeerTiers(selectors, {
        locked: lockfile.lockedVersions?.get(missingOptionalPeerName),
        lockfileOnly: lockfile.lockfileOnlyVersions?.get(missingOptionalPeerName),
      }),
      qualifies: (version) => (rootRange == null || semver.satisfies(version, rootRange)) &&
        ranges.every(range => semver.satisfies(version, range)) &&
        acceptsCandidate(missingOptionalPeerName, version),
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
