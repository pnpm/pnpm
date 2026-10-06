import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import * as dp from '@pnpm/deps.path'
import type {
  LockfileObject,
  PackageSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
  type PkgSnapshotToResolutionOptions,
} from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import {
  DIRECT_DEP_SELECTOR_WEIGHT,
  EXISTING_VERSION_SELECTOR_WEIGHT,
  type PkgResolutionId,
  type PreferredVersions,
  type VersionSelectors,
} from '@pnpm/resolving.resolver-base'
import type { RegistryContext } from '@pnpm/types'
import semver from 'semver'

import type { WantedDependency } from './getWantedDependencies.js'
import type {
  ExtendedWantedDependency,
  InfoFromLockfile,
  LockedPeerContext,
  PkgAddress,
  ResolutionContext,
} from './resolutionTypes.js'
import { unwrapPackageName } from './unwrapPackageName.js'

export type DepsToResolveOptions = RegistryContext & {
  currentDepth?: number
  directDepVersions?: Record<string, string[]>
  preferredDependencies?: ResolvedDependencies
  lockedDependencies?: ResolvedDependencies
  preferredVersions?: PreferredVersions
  prefix: string
  proceed: boolean
  resolvedDependencies?: ResolvedDependencies
  staleOverrideTargets?: ReadonlySet<string>
}

interface LockedReferenceLookup {
  currentDepth?: number
  directDepVersions?: Record<string, string[]>
  lockedDependencies: ResolvedDependencies
  preferredDependencies: ResolvedDependencies
  preferredVersions?: PreferredVersions
  resolvedDependencies: ResolvedDependencies
  satisfiesWantedSpec: (wantedDep: { alias: string, bareSpecifier: string }, preferredRef: string) => boolean
  staleOverrideTargets?: ReadonlySet<string>
  wantedLockfile: LockfileObject
}

interface LockedReferenceChoice {
  preferredVersion?: string
  proceed?: boolean
  reference?: string
}

export function getDepsToResolve (
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  wantedLockfile: LockfileObject,
  options: DepsToResolveOptions
): ExtendedWantedDependency[] {
  const lookup: LockedReferenceLookup = {
    currentDepth: options.currentDepth,
    directDepVersions: options.directDepVersions,
    lockedDependencies: options.lockedDependencies ?? {},
    preferredDependencies: options.preferredDependencies ?? {},
    preferredVersions: options.preferredVersions,
    resolvedDependencies: options.resolvedDependencies ?? {},
    satisfiesWantedSpec: referenceSatisfiesWantedSpec.bind(null, {
      lockfile: wantedLockfile,
      prefix: options.prefix,
    }),
    staleOverrideTargets: options.staleOverrideTargets,
    wantedLockfile,
  }
  const extendedWantedDeps: ExtendedWantedDependency[] = []
  // The only reason we resolve children in case the package depends on peers
  // is to get information about the existing dependencies, so that they can
  // be merged with the resolved peers.
  let proceedAll = options.proceed
  for (const wantedDependency of wantedDependencies) {
    const { reference, preferredVersion, proceed } = chooseLockedReference(wantedDependency, lookup)
    const infoFromLockfile = getInfoFromLockfile(wantedLockfile, pickRegistryContext(options), reference, wantedDependency.alias)
    if (!proceedAll && makesSiblingsProceed(wantedDependency, infoFromLockfile)) {
      proceedAll = true
      for (const extendedWantedDep of extendedWantedDeps) {
        extendedWantedDep.proceed = true
      }
    }
    extendedWantedDeps.push({
      infoFromLockfile,
      preferredVersion,
      proceed: proceedAll || proceed === true,
      wantedDependency,
    })
  }
  return extendedWantedDeps
}

function chooseLockedReference (
  wantedDependency: WantedDependency,
  lookup: LockedReferenceLookup
): LockedReferenceChoice {
  const { alias } = wantedDependency
  if (!alias || lookup.staleOverrideTargets?.has(alias)) return {}
  const satisfiesWanted = lookup.satisfiesWantedSpec.bind(null, { alias, bareSpecifier: wantedDependency.bareSpecifier })
  const pinnedRef = lookup.resolvedDependencies[alias]
  if (pinnedRef && (satisfiesWanted(pinnedRef) || pinnedRef.startsWith('file:'))) {
    return reuseOrRaisePinnedReference(wantedDependency, pinnedRef, lookup)
  }
  const preferredRef = lookup.preferredDependencies[alias]
  // If dependencies that were used by the previous version of the package
  // satisfy the newer version's requirements, then pnpm tries to keep
  // the previous dependency.
  // So for example, if foo@1.0.0 had bar@1.0.0 as a dependency
  // and foo was updated to 1.1.0 which depends on bar ^1.0.0
  // then bar@1.0.0 can be reused for foo@1.1.0
  if (semver.validRange(wantedDependency.bareSpecifier) !== null && preferredRef && satisfiesWanted(preferredRef)) {
    return { proceed: true, reference: preferredRef }
  }
  const lockedRef = lookup.lockedDependencies[alias]
  if (lockedRef && satisfiesWanted(lockedRef)) {
    return { preferredVersion: getPinnedNameVer(lookup.wantedLockfile, lockedRef, alias)?.version }
  }
  return {}
}

function reuseOrRaisePinnedReference (
  wantedDependency: WantedDependency,
  pinnedRef: string,
  lookup: LockedReferenceLookup
): LockedReferenceChoice {
  // Reusing a lockfile pin verbatim bypasses the preferred-versions
  // walk, so a transitive edge stays on a stale lower version even
  // when a direct dependency resolved to a higher version in range.
  const pinned = pinnedRef.startsWith('file:')
    ? undefined
    : getPinnedNameVer(lookup.wantedLockfile, pinnedRef, wantedDependency.alias!)
  const higherDirectVersion = pinned == null || lookup.currentDepth === 0
    ? undefined
    : findHigherDirectDepVersion({
      bareSpecifier: wantedDependency.bareSpecifier,
      directDepVersions: lookup.directDepVersions,
      pinned,
      preferredVersions: lookup.preferredVersions,
    })
  if (higherDirectVersion == null) return { reference: pinnedRef }
  // `preferredVersion` (singular) overrides the
  // EXISTING_VERSION_SELECTOR_WEIGHT stability bias that would
  // otherwise re-pick the lower version. The lower version is then
  // never resolved or fetched.
  return { proceed: true, preferredVersion: higherDirectVersion }
}

function makesSiblingsProceed (
  wantedDependency: WantedDependency,
  infoFromLockfile: InfoFromLockfile | undefined
): boolean {
  // A link into the declaring package has no lockfile entry and no
  // children, so it gives its siblings no reason to re-resolve.
  if (dp.packageRootLinkTarget(wantedDependency.bareSpecifier) != null) return false
  if (infoFromLockfile == null) return true
  const { dependencyLockfile } = infoFromLockfile
  return dependencyLockfile != null && (
    dependencyLockfile.peerDependencies != null ||
    Boolean(dependencyLockfile.transitivePeerDependencies?.length)
  )
}

function referenceSatisfiesWantedSpec (
  opts: {
    lockfile: LockfileObject
    prefix: string
  },
  wantedDep: { alias: string, bareSpecifier: string },
  preferredRef: string
): boolean {
  const depPath = dp.refToRelative(preferredRef, wantedDep.alias)
  if (depPath === null) return false
  const pkgSnapshot = opts.lockfile.packages?.[depPath]
  if (pkgSnapshot == null) {
    logger.warn({
      message: `Could not find preferred package ${depPath} in lockfile`,
      prefix: opts.prefix,
    })
    return false
  }
  const { name, version, registryName } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  const bareSpecifier = getRangeOfLockedPackage(wantedDep, { name, registryName })
  if (bareSpecifier == null) return false
  if (!semver.validRange(bareSpecifier) && Object.values(opts.lockfile.importers).filter(importer => importer.specifiers[wantedDep.alias] === wantedDep.bareSpecifier).length) {
    return true
  }
  return semver.satisfies(version, bareSpecifier, true)
}

/**
 * The part of the wanted specifier that the locked package's version is
 * checked against, or `undefined` when the locked package cannot satisfy the
 * wanted dependency whatever its version.
 */
function getRangeOfLockedPackage (
  wantedDep: { alias: string, bareSpecifier: string },
  { name, registryName }: { name: string, registryName?: string }
): string | undefined {
  const { bareSpecifier } = wantedDep
  if (registryName == null && bareSpecifier.startsWith('npm:')) {
    const npmAlias = unwrapPackageName(wantedDep.alias, bareSpecifier)
    return npmAlias.pkgName === name ? npmAlias.bareSpecifier : undefined
  }
  if (registryName == null) {
    // A range names the alias's own package. A git or tarball specifier may
    // resolve to a package of any name.
    return name === wantedDep.alias || semver.validRange(bareSpecifier) == null ? bareSpecifier : undefined
  }
  // A registry-qualified entry may only satisfy a spec of the same named
  // registry. A plain semver range means a default/scope-registry dep, which
  // the qualified entry must never be substituted for.
  if (!bareSpecifier.startsWith(`${registryName}:`)) return undefined
  // Reduce `<registryName>:[<name>@]<range>` to its range for the semver check.
  const body = bareSpecifier.slice(registryName.length + 1)
  const versionDelimiter = body.lastIndexOf('@')
  return versionDelimiter > 0 ? body.slice(versionDelimiter + 1) : body
}

/**
 * An edge without a lockfile entry of its own (such as an auto-installed peer)
 * may resolve to a package that the lockfile already has. That package keeps
 * the versions of its locked dependencies, as it does when it is reached
 * through a locked edge.
 */
export function getLockedDependenciesOfPkg (ctx: ResolutionContext, pkg: PkgAddress): ResolvedDependencies | undefined {
  const depPath = ctx.lockedDepPathByPkgId.get(pkg.pkgId)
  if (depPath == null) return undefined
  const snapshot = getInfoFromLockfile(ctx.wantedLockfile, pickRegistryContext(ctx), depPath, pkg.pkg.name)?.dependencyLockfile
  if (snapshot == null) return undefined
  return {
    ...snapshot.dependencies,
    ...snapshot.optionalDependencies,
  }
}

export function getPinnedNameVer (
  lockfile: LockfileObject,
  reference: string,
  alias: string
): { name: string, version: string } | undefined {
  const depPath = dp.refToRelative(reference, alias)
  if (depPath === null) return undefined
  const pkgSnapshot = lockfile.packages?.[depPath]
  if (pkgSnapshot == null) return undefined
  return nameVerFromPkgSnapshot(depPath, pkgSnapshot)
}

// The highest version a direct dependency (the only deterministic,
// resolved-first anchor) resolved to that is higher than the
// lockfile-pinned version and still satisfies the edge's range, or
// `undefined` when none exists.
function findHigherDirectDepVersion (
  { bareSpecifier, directDepVersions, pinned, preferredVersions }: {
    bareSpecifier: string
    directDepVersions: Record<string, string[]> | undefined
    pinned: { name: string, version: string }
    preferredVersions: PreferredVersions | undefined
  }
): string | undefined {
  if (!semver.valid(pinned.version)) return undefined
  if (semver.validRange(bareSpecifier) === null) return undefined
  const candidates = directDepVersions && Object.hasOwn(directDepVersions, pinned.name)
    ? directDepVersions[pinned.name] ?? []
    : getDirectDepVersionsFromSelectors(preferredVersions?.[pinned.name])
  let best: string | undefined
  for (const candidate of candidates) {
    if (
      semver.valid(candidate) &&
      semver.gt(candidate, pinned.version) &&
      semver.satisfies(candidate, bareSpecifier, true) &&
      (best == null || semver.gt(candidate, best))
    ) {
      best = candidate
    }
  }
  return best
}

function getDirectDepVersionsFromSelectors (selectors: VersionSelectors | undefined): string[] {
  if (selectors == null) return []
  return Object.entries(selectors)
    .filter(([, selector]) => typeof selector === 'object' &&
      selector !== null &&
      selector.selectorType === 'version' &&
      selector.weight >= DIRECT_DEP_SELECTOR_WEIGHT &&
      selector.weight < EXISTING_VERSION_SELECTOR_WEIGHT)
    .map(([candidate]) => candidate)
}

function getLockedPeerContext (dependencyLockfile: PackageSnapshot): LockedPeerContext | undefined {
  if (dependencyLockfile.peerDependencies == null) return undefined
  const lockedPeerContext: LockedPeerContext = {}
  for (const peerName of Object.keys(dependencyLockfile.peerDependencies)) {
    const ref = dependencyLockfile.dependencies?.[peerName] ?? dependencyLockfile.optionalDependencies?.[peerName]
    const depPath = ref == null ? null : dp.refToRelative(ref, peerName)
    if (depPath != null) lockedPeerContext[peerName] = depPath
  }
  return Object.keys(lockedPeerContext).length === 0 ? undefined : lockedPeerContext
}

export function getInfoFromLockfile (
  lockfile: LockfileObject,
  registryOpts: PkgSnapshotToResolutionOptions,
  reference: string | undefined,
  alias: string | undefined
): InfoFromLockfile | undefined {
  if (!reference || !alias) {
    return undefined
  }

  const depPath = dp.refToRelative(reference, alias)

  if (!depPath) {
    return undefined
  }

  const dependencyLockfile = lockfile.packages?.[depPath]
  if (dependencyLockfile == null) {
    return {
      depPath,
      pkgId: getPkgIdOfDepPath(depPath), // Does it make sense to set pkgId when we're not sure?
    }
  }
  return getInfoFromPackageSnapshot(depPath, dependencyLockfile, registryOpts)
}

function getInfoFromPackageSnapshot (
  depPath: InfoFromLockfile['depPath'],
  pkgSnapshot: PackageSnapshot,
  registryOpts: PkgSnapshotToResolutionOptions
): InfoFromLockfile {
  const lockedPeerContext = getLockedPeerContext(pkgSnapshot)
  const dependencyLockfile = omitPeersFromSnapshotDependencies(pkgSnapshot)
  const { name, version, nonSemverVersion, registryName } = nameVerFromPkgSnapshot(depPath, dependencyLockfile)
  return {
    depPath,
    name,
    version,
    dependencyLockfile,
    lockedPeerContext,
    pkgId: nonSemverVersion ?? (registryName ? `${name}@${registryName}:${version}` : `${name}@${version}`) as PkgResolutionId,
    // resolution may not exist if lockfile is broken, and an unexpected error will be thrown
    // if resolution does not exist, return undefined so it can be autofixed later
    resolution: dependencyLockfile.resolution && pkgSnapshotToResolution(depPath, dependencyLockfile, registryOpts),
  }
}

function omitPeersFromSnapshotDependencies (pkgSnapshot: PackageSnapshot): PackageSnapshot {
  const { peerDependencies } = pkgSnapshot
  if ((peerDependencies == null) || (pkgSnapshot.dependencies == null)) return pkgSnapshot
  // This is done to guarantee that the dependency will be relinked with the
  // up-to-date peer dependencies
  // Covered by test: "peer dependency is grouped with dependency when peer is resolved not from a top dependency"
  const dependencies: Record<string, string> = {}
  for (const [depName, ref] of Object.entries(pkgSnapshot.dependencies)) {
    if (peerDependencies[depName]) continue
    dependencies[depName] = ref
  }
  return {
    ...pkgSnapshot,
    dependencies,
  }
}

function getPkgIdOfDepPath (depPath: string): PkgResolutionId {
  const parsed = dp.parse(depPath)
  if (parsed.nonSemverVersion != null) return parsed.nonSemverVersion as PkgResolutionId
  if (parsed.name && parsed.version) {
    return (parsed.registryName ? `${parsed.name}@${parsed.registryName}:${parsed.version}` : `${parsed.name}@${parsed.version}`) as PkgResolutionId
  }
  return depPath as PkgResolutionId
}
