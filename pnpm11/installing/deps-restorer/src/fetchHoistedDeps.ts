import path from 'node:path'

import { pickRegistryContext } from '@pnpm/config.normalize-registries'
import { packageIsInstallable } from '@pnpm/config.package-is-installable'
import type { DependenciesGraph, DepHierarchy } from '@pnpm/deps.graph-builder'
import * as dp from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import { getHoisterPkgId, type HoisterResult } from '@pnpm/installing.linking.real-hoist'
import type { IncludedDependencies } from '@pnpm/installing.modules-yaml'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { getPatchInfo } from '@pnpm/patching.config'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { FetchPackageToStoreFunction } from '@pnpm/store.controller-types'
import type { DepPath } from '@pnpm/types'
import { pathExists } from 'path-exists'

import type { LockfileToHoistedDepGraphOptions } from './lockfileToHoistedDepGraph.js'

export interface SkipFetchingOption {
  /**
   * Build the graph without reaching the store. The previous graph is
   * only diffed by directory name, and its forced walk visits packages
   * the earlier install skipped and never downloaded.
   */
  skipFetching?: boolean
}

export type FetchDepsOptions = {
  graph: DependenciesGraph
  lockfile: LockfileObject
  /**
   * Every directory a package landed in, in visit order; the first
   * entry wins for parent → child wiring. Keyed by
   * {@link getHoisterPkgId}, not depPath: the hoister collapses
   * every peer variant of one version onto one node, so only the
   * first variant's depPath reaches this walk. Sharing the
   * hoister's own identity function is what lets an edge declared
   * against another variant still find the copy that survived.
   */
  pkgLocationsByPkgId: Record<string, string[]>
  injectionTargetsByDepPath: Map<string, string[]>
  hoistedLocations: Record<string, string[]>
} & LockfileToHoistedDepGraphOptions & SkipFetchingOption

type FetchResponse = ReturnType<FetchPackageToStoreFunction>

/** A hoisted package that is installable and gets a directory in the graph. */
interface PlacedDep {
  dep: HoisterResult
  depPath: DepPath
  pkgSnapshot: PackageSnapshot
  pkgName: string
  pkgVersion: string
  packageId: ReturnType<typeof packageIdFromSnapshot>
  dir: string
  /** `dir` relative to the lockfile directory. */
  depLocation: string
  /** The `node_modules` directory that holds `dir`. */
  modules: string
}

export async function fetchDeps (
  opts: FetchDepsOptions,
  modules: string,
  deps: Set<HoisterResult>
): Promise<DepHierarchy> {
  const depHierarchy: Record<string, DepHierarchy> = {}
  await Promise.all(Array.from(deps).map(async (dep) => {
    const placedDep = placeDep(opts, modules, dep)
    if (placedDep == null) return
    // Awaited only when pending: a package that needs no waiting is added to the
    // graph, and its subtree walked, before the next sibling is looked at.
    let fetchResponse = fetchPlacedDep(opts, placedDep)
    if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
    if (fetchResponse == null) return
    addGraphNode(opts, placedDep, fetchResponse)
    const { dir, depPath, pkgSnapshot } = placedDep
    depHierarchy[dir] = await fetchDeps(opts, path.join(dir, 'node_modules'), dep.dependencies)
    pushLocation(opts.hoistedLocations, depPath, placedDep.depLocation)
    opts.graph[dir].children = getChildren(pkgSnapshot, opts.pkgLocationsByPkgId, opts)
    if (!opts.skipFetching) {
      opts.graph[dir].packageRootLinks = getPackageRootLinks(pkgSnapshot, opts.include)
    }
  }))
  return depHierarchy
}

/**
 * Where the hoisted package goes, or `undefined` when it is left out: a skipped
 * or workspace package, a link, an incompatible package, or an ignored local one.
 */
function placeDep (opts: FetchDepsOptions, modules: string, dep: HoisterResult): PlacedDep | undefined {
  const depPath = Array.from(dep.references)[0] as DepPath
  if (opts.skipped.has(depPath) || depPath.startsWith('workspace:')) return undefined
  const pkgSnapshot = opts.lockfile.packages![depPath]
  if (!pkgSnapshot) {
    // it is a link
    return undefined
  }
  const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  const packageId = packageIdFromSnapshot(depPath, pkgSnapshot)

  if (!opts.includeIncompatiblePackages && !isDepInstallable(opts, { depPath, packageId, pkgName, pkgSnapshot, pkgVersion })) {
    opts.skipped.add(depPath)
    return undefined
  }

  const isDirectoryDep = 'directory' in pkgSnapshot.resolution && pkgSnapshot.resolution.directory != null
  if (isDirectoryDep && opts.ignoreLocalPackages) {
    logger.info({
      message: `Skipping local dependency ${pkgName}@${pkgVersion} (file: protocol)`,
      prefix: opts.lockfileDir,
    })
    return undefined
  }

  const dir = safeJoinModulesDir(modules, dep.name)
  const depLocation = path.relative(opts.lockfileDir, dir)
  return { dep, depLocation, depPath, dir, modules, packageId, pkgName, pkgSnapshot, pkgVersion }
}

function isDepInstallable (
  opts: FetchDepsOptions,
  { depPath, packageId, pkgName, pkgSnapshot, pkgVersion }: Pick<PlacedDep, 'depPath' | 'packageId' | 'pkgName' | 'pkgSnapshot' | 'pkgVersion'>
): boolean {
  const pkg = {
    name: pkgName,
    version: pkgVersion,
    engines: opts.engineStrict && dp.hasPatchHash(depPath) ? undefined : pkgSnapshot.engines,
    cpu: pkgSnapshot.cpu,
    os: pkgSnapshot.os,
    libc: pkgSnapshot.libc,
  }
  return packageIsInstallable(packageId, pkg, {
    // An incompatibility inside an `optionalDependencies` subtree is
    // reported, not fatal — see `filterLockfileByImportersAndEngine`,
    // which classifies these dep paths.
    engineStrict: opts.engineStrict && pkgSnapshot.optional !== true,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    optional: !opts.requiredDepPaths.has(depPath),
    supportedArchitectures: opts.supportedArchitectures,
  }) !== false
}

/**
 * The store's response for the package, or `undefined` when an optional
 * package failed to fetch.
 */
function fetchPlacedDep (
  opts: FetchDepsOptions,
  placedDep: PlacedDep
): FetchResponse | undefined | Promise<FetchResponse | undefined> {
  if (opts.skipFetching) {
    return {} as unknown as FetchResponse
  }
  // We check for the existence of the package inside node_modules.
  // It will only be missing if the user manually removed it.
  // That shouldn't normally happen but Bit CLI does remove node_modules in component directories:
  // https://github.com/teambit/bit/blob/5e1eed7cd122813ad5ea124df956ee89d661d770/scopes/dependencies/dependency-resolver/dependency-installer.ts#L169
  //
  // We also verify that the package that is present has the expected version.
  // This check is required because there is no guarantee the modules manifest and current lockfile were
  // successfully saved after node_modules was changed during installation.
  if (!opts.currentHoistedLocations?.[placedDep.depPath]?.includes(placedDep.depLocation)) {
    return requestPackage(opts, placedDep)
  }
  return reuseOrRequestPackage(opts, placedDep)
}

async function reuseOrRequestPackage (opts: FetchDepsOptions, placedDep: PlacedDep): Promise<FetchResponse | undefined> {
  if (!await dirHasPackageJsonWithVersion(path.join(opts.lockfileDir, placedDep.depLocation), placedDep.pkgVersion)) {
    return requestPackage(opts, placedDep)
  }
  if (mayDelegateToDirectory(opts, placedDep.pkgSnapshot.resolution)) {
    return refetchPresentPackage(opts, placedDep)
  }
  const { filesIndexFile } = opts.storeController.getFilesIndexFilePath({
    ignoreScripts: opts.ignoreScripts,
    pkg: toPkgResolution(opts, placedDep),
  })
  return { filesIndexFile } as unknown as FetchResponse
}

/**
 * Whether a custom fetcher may serve the resolution from a mutable local
 * directory. The installed version alone cannot tell whether such a source
 * changed since the last install. An integrity-pinned archive cannot change.
 */
function mayDelegateToDirectory (opts: FetchDepsOptions, resolution: PackageSnapshot['resolution']): boolean {
  if (opts.storeController.hasCustomFetchers !== true) return false
  const isArchive = !('type' in resolution) || resolution.type == null || resolution.type === 'binary'
  const hasIntegrity = 'integrity' in resolution && typeof resolution.integrity === 'string' && resolution.integrity.length > 0
  return !(isArchive && hasIntegrity)
}

/**
 * Fetches a package that is already installed. Only files fetched from a
 * local directory are imported again. Any other source cannot have changed,
 * so the installed copy is kept.
 */
async function refetchPresentPackage (opts: FetchDepsOptions, placedDep: PlacedDep): Promise<FetchResponse | undefined> {
  const fetchResponse = await requestPackage(opts, placedDep)
  if (fetchResponse == null) return undefined
  let resolvedFrom: string
  try {
    resolvedFrom = (await fetchResponse.fetching()).files.resolvedFrom
  } catch (err: unknown) {
    if (placedDep.pkgSnapshot.optional) return undefined
    throw err
  }
  if (resolvedFrom === 'local-dir') return fetchResponse
  return { filesIndexFile: fetchResponse.filesIndexFile } as unknown as FetchResponse
}

function requestPackage (
  opts: FetchDepsOptions,
  placedDep: PlacedDep
): FetchResponse | undefined | Promise<FetchResponse | undefined> {
  let fetchResponse: FetchResponse
  try {
    fetchResponse = opts.storeController.fetchPackage({
      allowBuild: opts.allowBuild,
      force: false,
      lockfileDir: opts.lockfileDir,
      ignoreScripts: opts.ignoreScripts,
      pkg: toPkgResolution(opts, placedDep),
      supportedArchitectures: opts.supportedArchitectures,
    }) as unknown as FetchResponse
  } catch (err: unknown) {
    if (placedDep.pkgSnapshot.optional) return undefined
    throw err
  }
  if (!(fetchResponse instanceof Promise)) return fetchResponse
  return (fetchResponse as Promise<FetchResponse>).catch((err: unknown) => {
    if (placedDep.pkgSnapshot.optional) return undefined
    throw err
  })
}

function toPkgResolution (opts: FetchDepsOptions, { depPath, packageId, pkgName, pkgSnapshot, pkgVersion }: PlacedDep) {
  return {
    id: packageId,
    resolution: pkgSnapshotToResolution(depPath, pkgSnapshot, pickRegistryContext(opts)),
    name: pkgName,
    version: pkgVersion,
  }
}

function addGraphNode (opts: FetchDepsOptions, placedDep: PlacedDep, fetchResponse: FetchResponse): void {
  const { dep, depPath, dir, modules, pkgName, pkgSnapshot, pkgVersion } = placedDep
  opts.graph[dir] = {
    alias: dep.name,
    children: {},
    depPath,
    pkgIdWithPatchHash: dp.getPkgIdWithPatchHash(depPath),
    dir,
    fetching: fetchResponse.fetching,
    filesIndexFile: fetchResponse.filesIndexFile,
    hasBin: pkgSnapshot.hasBin === true,
    hasBundledDependencies: pkgSnapshot.bundledDependencies != null,
    modules,
    name: pkgName,
    version: pkgVersion,
    optional: !!pkgSnapshot.optional,
    optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
    patch: getPatchInfo(opts.patchedDependencies, pkgName, pkgVersion),
    resolution: pkgSnapshot.resolution,
  }
  pushLocation(opts.pkgLocationsByPkgId, getHoisterPkgId(depPath, pkgSnapshot), dir)
  if ('directory' in pkgSnapshot.resolution && pkgSnapshot.resolution.directory != null) {
    const locations = opts.injectionTargetsByDepPath.get(depPath)
    if (locations) {
      locations.push(dir)
    } else {
      opts.injectionTargetsByDepPath.set(depPath, [dir])
    }
  }
}

function pushLocation (locationsByKey: Record<string, string[]>, key: string, location: string): void {
  if (!locationsByKey[key]) {
    locationsByKey[key] = []
  }
  locationsByKey[key].push(location)
}

async function dirHasPackageJsonWithVersion (dir: string, expectedVersion?: string): Promise<boolean> {
  if (!expectedVersion) return pathExists(dir)
  try {
    const manifest = await safeReadPackageJsonFromDir(dir)
    return manifest?.version === expectedVersion
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException)?.code === 'ENOENT') {
      return pathExists(dir)
    }
    throw err
  }
}

function getPackageRootLinks (pkgSnapshot: PackageSnapshot, include: IncludedDependencies): Record<string, string> | undefined {
  let links: Record<string, string> | undefined
  const allDeps = {
    ...pkgSnapshot.dependencies,
    ...(include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
  }
  for (const [alias, ref] of Object.entries(allDeps)) {
    const target = dp.packageRootLinkTarget(ref)
    if (target != null) {
      links ??= {}
      links[alias] = target
    }
  }
  return links
}

function getChildren (
  pkgSnapshot: PackageSnapshot,
  pkgLocationsByPkgId: Record<string, string[]>,
  opts: { include: IncludedDependencies, lockfile: LockfileObject }
): Record<string, string> {
  const allDeps = {
    ...pkgSnapshot.dependencies,
    ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
  }
  const children: Record<string, string> = {}
  for (const [childName, childRef] of Object.entries(allDeps)) {
    const childDepPath = dp.refToRelative(childRef, childName)
    if (!childDepPath) continue
    const childSnapshot = opts.lockfile.packages?.[childDepPath]
    if (!childSnapshot) continue
    const locations = pkgLocationsByPkgId[getHoisterPkgId(childDepPath, childSnapshot)]
    if (locations) {
      children[childName] = locations[0]
    }
  }
  return children
}
