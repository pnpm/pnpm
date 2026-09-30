import type { PreferredVersions } from '@pnpm/resolving.resolver-base'

import type {
  PkgAddress,
  PostponedPeersResolutionFunction,
  PostponedResolutionFunction,
  ResolutionContext,
  ResolveDependenciesOfDependency,
  ResolvedPackage,
} from './resolutionTypes.js'

export interface CollectedDependencies {
  pkgAddresses: PkgAddress[]
  postponedResolutionsQueue: PostponedResolutionFunction[]
  postponedPeersResolutionQueue: PostponedPeersResolutionFunction[]
}

/**
 * Drains the results in input order. Pushing from inside the Promise.all
 * callbacks that produce them would leak completion-order timing into
 * pkgAddresses / postponedResolutionsQueue, which downstream determines how
 * cyclic peer suffixes are assigned. See pnpm/pnpm#8155.
 */
export function collectResolvedDependencies (resolvedDependencies: ResolveDependenciesOfDependency[]): CollectedDependencies {
  const collected: CollectedDependencies = {
    pkgAddresses: [],
    postponedResolutionsQueue: [],
    postponedPeersResolutionQueue: [],
  }
  for (const { resolveDependencyResult, postponedResolution, postponedPeersResolution } of resolvedDependencies) {
    if (resolveDependencyResult) {
      collected.pkgAddresses.push(resolveDependencyResult as PkgAddress)
    }
    if (postponedResolution) {
      collected.postponedResolutionsQueue.push(postponedResolution)
    }
    if (postponedPeersResolution) {
      collected.postponedPeersResolutionQueue.push(postponedPeersResolution)
    }
  }
  return collected
}

export interface IndexResolvedDependenciesOptions {
  /** Collects the versions each dependency resolved to, by package name. */
  directDepVersions?: Record<string, string[]>
  /** Adds the aliases of updated dependencies to `ctx.updatedSet`. */
  markUpdated: boolean
  preferredVersions: PreferredVersions
  /** The weight the resolved version gets on top of any selector it already has. */
  selectorWeight: number
}

export interface IndexedDependencies {
  currentParentPkgAliases: Record<string, PkgAddress | true>
  newPreferredVersions: PreferredVersions
}

/**
 * Indexes a set of sibling dependencies for the resolution of their
 * children: by alias, for the lookup of peers, and by version, so that the
 * children prefer the versions their parents already resolved to.
 */
export function indexResolvedDependencies (
  ctx: Pick<ResolutionContext, 'resolvedPkgsById' | 'updatedSet'>,
  pkgAddresses: PkgAddress[],
  opts: IndexResolvedDependenciesOptions
): IndexedDependencies {
  const newPreferredVersions = Object.create(opts.preferredVersions) as PreferredVersions
  const currentParentPkgAliases: Record<string, PkgAddress | true> = {}
  for (const pkgAddress of pkgAddresses) {
    if (currentParentPkgAliases[pkgAddress.alias] !== true) {
      currentParentPkgAliases[pkgAddress.alias] = pkgAddress
    }
    if (pkgAddress.updated && opts.markUpdated) {
      ctx.updatedSet.add(pkgAddress.alias)
    }
    const resolvedPackage = ctx.resolvedPkgsById[pkgAddress.pkgId]
    if (!resolvedPackage) continue // This will happen only with linked dependencies
    if (opts.directDepVersions) {
      addDirectDepVersion(opts.directDepVersions, resolvedPackage)
    }
    addPreferredVersionSelector(newPreferredVersions, resolvedPackage, opts)
  }
  return { currentParentPkgAliases, newPreferredVersions }
}

export function addDirectDepVersion (
  directDepVersions: Record<string, string[]>,
  { name, version }: Pick<ResolvedPackage, 'name' | 'version'>
): void {
  directDepVersions[name] ??= []
  if (!directDepVersions[name].includes(version)) {
    directDepVersions[name].push(version)
  }
}

function addPreferredVersionSelector (
  newPreferredVersions: PreferredVersions,
  { name, version }: Pick<ResolvedPackage, 'name' | 'version'>,
  { preferredVersions, selectorWeight }: Pick<IndexResolvedDependenciesOptions, 'preferredVersions' | 'selectorWeight'>
): void {
  if (!Object.hasOwn(newPreferredVersions, name)) {
    newPreferredVersions[name] = { ...preferredVersions[name] }
  }
  const existingSelector = newPreferredVersions[name][version]
  const existingWeight = typeof existingSelector === 'object' && existingSelector != null
    ? existingSelector.weight
    : typeof existingSelector === 'string' ? 1 : 0
  newPreferredVersions[name][version] = {
    selectorType: 'version',
    weight: existingWeight + selectorWeight,
  }
}
