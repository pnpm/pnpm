import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import { DIRECT_DEP_SELECTOR_WEIGHT } from '@pnpm/resolving.resolver-base'
import { zipWith } from 'ramda'
import semver from 'semver'

import { getDepsToResolve } from './getDepsToResolve.js'
import { type CollectedDependencies, collectResolvedDependencies, indexResolvedDependencies } from './indexResolvedDependencies.js'
import { getCatalogExistingVersionFromSnapshot, lookUpCatalogEntry } from './lookUpCatalogEntry.js'
import { startResolvingPeers } from './missingPeers.js'
import type {
  ExtendedWantedDependency,
  ImporterToResolve,
  PkgAddress,
  PkgAddressesByImportersWithoutPeers,
  ResolutionContext,
  ResolveDependenciesOfDependency,
  ResolveDependenciesOfImportersResult,
} from './resolutionTypes.js'
import { resolveDependenciesOfDependency } from './resolveDependencies.js'

export async function resolveDependenciesOfImporters (
  ctx: ResolutionContext,
  importers: ImporterToResolve[]
): Promise<ResolveDependenciesOfImportersResult> {
  const pickLowestVersion = ctx.resolutionMode === 'time-based' || ctx.resolutionMode === 'lowest-direct'
  const resolveResults = await Promise.all(
    importers.map(async (importer) => resolveDirectDependenciesOfImporter(ctx, { importer, pickLowestVersion }))
  )
  const { publishedBy, time } = getPublishedByCutoff(ctx, resolveResults)
  const pkgAddressesByImportersWithoutPeers = await Promise.all(zipWith(
    async (importer, directDependencies) => resolveSubdependenciesOfImporter(ctx, { directDependencies, importer, publishedBy }),
    importers,
    resolveResults
  ))
  return {
    pkgAddressesByImportersWithoutPeers,
    publishedBy,
    time,
  }
}

async function resolveDirectDependenciesOfImporter (
  ctx: ResolutionContext,
  { importer, pickLowestVersion }: { importer: ImporterToResolve, pickLowestVersion: boolean }
): Promise<CollectedDependencies> {
  const extendedWantedDeps = getDepsToResolve(importer.wantedDependencies, ctx.wantedLockfile, {
    currentDepth: 0,
    preferredDependencies: importer.options.preferredDependencies,
    prefix: importer.options.prefix,
    proceed: importer.options.proceed || ctx.forceFullResolution,
    ...pickRegistryContext(ctx),
    resolvedDependencies: importer.options.resolvedDependencies,
    staleOverrideTargets: ctx.staleOverrideTargets,
  })
  const resolveDependenciesOfImporterWantedDep = resolveDependenciesOfImporterDependency.bind(null, {
    ctx,
    importer,
    pickLowestVersion,
  })
  return collectResolvedDependencies(await Promise.all(extendedWantedDeps.map(resolveDependenciesOfImporterWantedDep)))
}

function getPublishedByCutoff (
  ctx: ResolutionContext,
  resolveResults: CollectedDependencies[]
): { publishedBy?: Date, time?: Record<string, string> } {
  let publishedBy: Date | undefined
  let time: Record<string, string> | undefined
  if (ctx.resolutionMode === 'time-based') {
    const result = getPublishedByDate(resolveResults.map(({ pkgAddresses }) => pkgAddresses).flat(), ctx.wantedLockfile.time)
    if (result.publishedBy) {
      publishedBy = new Date(result.publishedBy.getTime() + 60 * 60 * 1000) // adding 1 hour delta
      time = result.newTime
    }
  }
  if (ctx.maximumPublishedBy && (publishedBy == null || publishedBy > ctx.maximumPublishedBy)) {
    publishedBy = ctx.maximumPublishedBy
  }
  return { publishedBy, time }
}

async function resolveSubdependenciesOfImporter (
  ctx: ResolutionContext,
  { directDependencies, importer, publishedBy }: {
    directDependencies: CollectedDependencies
    importer: ImporterToResolve
    publishedBy?: Date
  }
): Promise<PkgAddressesByImportersWithoutPeers> {
  const { pkgAddresses, postponedResolutionsQueue, postponedPeersResolutionQueue } = directDependencies
  const directDepVersions: Record<string, string[]> = Object.create(null)
  const { currentParentPkgAliases, newPreferredVersions } = indexResolvedDependencies(ctx, pkgAddresses, {
    directDepVersions,
    markUpdated: true,
    preferredVersions: importer.preferredVersions,
    selectorWeight: DIRECT_DEP_SELECTOR_WEIGHT,
  })
  importer.options.directDepVersions = directDepVersions
  const postponedResolutionOpts = {
    directDepVersions,
    preferredVersions: newPreferredVersions,
    parentPkgAliases: { ...importer.parentPkgAliases, ...currentParentPkgAliases },
    publishedBy,
  }
  const childrenResults = await Promise.all(
    postponedResolutionsQueue.map((postponedResolution) => postponedResolution(postponedResolutionOpts))
  )
  if (!ctx.hoistPeers) {
    return {
      missingPeers: {},
      pkgAddresses,
      resolvedPeers: {},
    }
  }
  const { missingPeers, resolvedPeers } = await startResolvingPeers({
    childrenResults,
    pkgAddresses,
    parentPkgAliases: postponedResolutionOpts.parentPkgAliases,
    currentParentPkgAliases,
    postponedPeersResolutionQueue,
    autoInstallPeersFromHighestMatch: ctx.autoInstallPeersFromHighestMatch,
  })
  return {
    missingPeers,
    pkgAddresses,
    resolvedPeers,
  }
}

export interface ResolveDependenciesOfImporterDependencyOpts {
  readonly ctx: ResolutionContext
  readonly importer: ImporterToResolve
  readonly pickLowestVersion: boolean
}

async function resolveDependenciesOfImporterDependency (
  { ctx, importer, pickLowestVersion }: ResolveDependenciesOfImporterDependencyOpts,
  extendedWantedDep: ExtendedWantedDependency
): Promise<ResolveDependenciesOfDependency> {
  // The catalog protocol is only usable in importers (i.e. packages in the
  // workspace. Replacing catalog protocol while resolving importers here before
  // resolving dependencies of packages outside of the workspace/monorepo.
  const originalBareSpecifier = extendedWantedDep.wantedDependency.bareSpecifier
  const originalPrevSpecifier = extendedWantedDep.wantedDependency.prevSpecifier
  const catalogSpecifier = originalPrevSpecifier != null &&
    parseCatalogProtocol(originalPrevSpecifier) != null &&
    isExplicitDistTagSpecifier(originalBareSpecifier)
    ? originalPrevSpecifier
    : originalBareSpecifier
  const catalogLookup = lookUpCatalogEntry(ctx.catalogResolver, {
    ...extendedWantedDep.wantedDependency,
    bareSpecifier: catalogSpecifier,
  })

  // The lockfile from a previous installation may have already resolved this
  // cataloged dependency. Reuse the exact version in the lockfile catalog
  // snapshot to ensure all projects using the same cataloged dependency get the
  // same version.
  if (catalogLookup != null && originalBareSpecifier === catalogSpecifier) {
    extendedWantedDep.wantedDependency.bareSpecifier = catalogLookup.specifier
    extendedWantedDep.preferredVersion = getCatalogExistingVersionFromSnapshot(catalogLookup, ctx.wantedLockfile, extendedWantedDep.wantedDependency)
  }

  const result = await resolveDependenciesOfDependency(
    ctx,
    importer.preferredVersions,
    {
      ...importer.options,
      parentPkgAliases: importer.parentPkgAliases,
      pickLowestVersion: pickLowestVersion && !importer.updatePackageManifest,
      rangeSpecStyle: importer.rangeSpecStyle,
      publishedBy: ctx.maximumPublishedBy,
    },
    extendedWantedDep
  )

  // If the catalog protocol was used, store metadata about the catalog
  // lookup to use in the lockfile.
  if (result.resolveDependencyResult != null && catalogLookup != null) {
    result.resolveDependencyResult.catalogLookup = {
      ...catalogLookup,
      userSpecifiedBareSpecifier: catalogSpecifier,
    }
  }

  return result
}

function isExplicitDistTagSpecifier (bareSpecifier: string | undefined): boolean {
  return bareSpecifier != null && bareSpecifier !== 'latest' && !bareSpecifier.includes(':') && semver.validRange(bareSpecifier) == null
}

function getPublishedByDate (pkgAddresses: PkgAddress[], timeFromLockfile: Record<string, string> = {}): { publishedBy: Date, newTime: Record<string, string> } {
  const newTime: Record<string, string> = {}
  for (const pkgAddress of pkgAddresses) {
    if (pkgAddress.publishedAt) {
      newTime[pkgAddress.pkgId] = pkgAddress.publishedAt
    } else if (timeFromLockfile[pkgAddress.pkgId]) {
      newTime[pkgAddress.pkgId] = timeFromLockfile[pkgAddress.pkgId]
    }
  }
  const sortedDates = Object.values(newTime)
    .map((publishedAt: string) => new Date(publishedAt))
    .sort((d1, d2) => d1.getTime() - d2.getTime())
  return { publishedBy: sortedDates[sortedDates.length - 1], newTime }
}
