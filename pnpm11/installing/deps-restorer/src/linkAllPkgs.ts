import { promises as fs } from 'node:fs'
import path from 'node:path'

import { lockGlobalVirtualStoreSlot } from '@pnpm/building.during-install'
import { reportPackageImported } from '@pnpm/core-loggers'
import type { DependenciesGraph, DependenciesGraphNode } from '@pnpm/deps.graph-builder'
import { calcDepState, type DepsStateCache, shouldIncludeDepGraphHash } from '@pnpm/deps.graph-hasher'
import { symlinkDependency } from '@pnpm/fs.symlink-dependency'
import { logger } from '@pnpm/logger'
import { createRemoteSideEffectsRestorer } from '@pnpm/pnpr.client'
import type {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import { pathExists } from 'path-exists'
import { isEmpty } from 'ramda'

import { limitLinking } from './limits.js'
import { reportOptionalFetchFailure } from './reportOptionalFetchFailure.js'

export interface LinkAllPkgsOptions {
  allowBuild?: AllowBuild
  depGraph: DependenciesGraph
  depsStateCache: DepsStateCache
  deferDependencyBuilds: boolean
  disableRelinkLocalDirDeps?: boolean
  enableGlobalVirtualStore?: boolean
  force: boolean
  ignoreScripts: boolean
  lockfileDir: string
  /**
   * The root project's `engines.runtime` Node version, which keys the
   * side-effects cache of every package that does not pin its own.
   */
  nodeVersion?: string
  sideEffectsCacheRead: boolean
  remoteSideEffectsCache?: RemoteSideEffectsCacheSettings
  pnprServer?: string
  configByUri: Record<string, RegistryConfig>
  storeDir: string
  supportedArchitectures?: SupportedArchitectures
}

interface PackageImportContext {
  needsBuildMarkerSrc?: string
  opts: LinkAllPkgsOptions
  restorer: ReturnType<typeof createRemoteSideEffectsRestorer>
  storeController: StoreController
}

type SideEffectsCacheKey = Awaited<ReturnType<NonNullable<PackageImportContext['restorer']>['restore']>>
type ImportPackageResult = Awaited<ReturnType<StoreController['importPackage']>>

/**
 * Imports every fetched package into its slot. Resolves to the directories
 * of the optional packages that could not be fetched and were left out.
 */
export async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: LinkAllPkgsOptions
): Promise<Set<string>> {
  let needsBuildMarkerSrc: string | undefined
  if (opts.enableGlobalVirtualStore) {
    needsBuildMarkerSrc = path.join(opts.storeDir, '.pnpm-needs-build-marker')
    await fs.writeFile(needsBuildMarkerSrc, '')
  }
  const restorer = createRemoteSideEffectsRestorer({
    allowBuild: opts.allowBuild,
    configByUri: opts.configByUri,
    depsGraph: opts.depGraph,
    depsStateCache: opts.depsStateCache,
    ignoreScripts: opts.ignoreScripts,
    nodeVersion: opts.nodeVersion,
    pnprServer: opts.pnprServer,
    settings: opts.remoteSideEffectsCache,
    sideEffectsCacheRead: opts.sideEffectsCacheRead,
    storeController,
    supportedArchitectures: opts.supportedArchitectures,
    warn: (message) => logger.warn({ message, prefix: opts.lockfileDir }),
  })
  const importContext: PackageImportContext = { needsBuildMarkerSrc, opts, restorer, storeController }
  const fetchFailedDirs = new Set<string>()
  await Promise.all(depNodes.map(async (depNode) => {
    if (!await importDepNode(depNode, importContext)) {
      fetchFailedDirs.add(depNode.dir)
    }
  }))
  return fetchFailedDirs
}

/** Resolves to `false` when the package is optional and could not be fetched. */
async function importDepNode (depNode: DependenciesGraphNode, importContext: PackageImportContext): Promise<boolean> {
  if (!depNode.fetching) return true
  const filesResponse = await fetchFilesResponse(depNode, importContext.opts.lockfileDir)
  if (filesResponse == null) return false
  depNode.requiresBuild = filesResponse.requiresBuild
  const sideEffectsCacheKey = await findSideEffectsCacheKey(depNode, filesResponse, importContext)
  const imported = await importIntoSlot(depNode, {
    filesResponse: addNeedsBuildMarker(filesResponse, { depNode, needsBuildMarkerSrc: importContext.needsBuildMarkerSrc, sideEffectsCacheKey }),
    sideEffectsCacheKey,
  }, importContext)
  if (imported?.importMethod) {
    reportPackageImported({
      method: imported.importMethod,
      requester: importContext.opts.lockfileDir,
      to: depNode.dir,
    })
  }
  if (imported != null) depNode.isBuilt = imported.isBuilt
  await linkSelfDependency(depNode, importContext.opts.depGraph)
  return true
}

/** Resolves to `undefined` when the fetch of an optional package failed. */
async function fetchFilesResponse (
  depNode: DependenciesGraphNode,
  lockfileDir: string
): Promise<PackageFilesResponse | undefined> {
  try {
    return (await depNode.fetching!()).files
  } catch (err: unknown) {
    if (!depNode.optional) throw err
    reportOptionalFetchFailure(err, depNode, lockfileDir)
    return undefined
  }
}

async function findSideEffectsCacheKey (
  depNode: DependenciesGraphNode,
  filesResponse: PackageFilesResponse,
  { opts, restorer }: PackageImportContext
): Promise<SideEffectsCacheKey> {
  const remoteCacheKey = await restorer?.restore({
    graphKey: depNode.dir,
    depPath: depNode.depPath,
    files: filesResponse,
    filesIndexFile: depNode.filesIndexFile,
    name: depNode.name,
    patchFileHash: depNode.patch?.hash,
    resolution: depNode.resolution,
    version: depNode.version,
  })
  if (remoteCacheKey != null) return remoteCacheKey
  const hasLocalSideEffects = opts.sideEffectsCacheRead && filesResponse.sideEffectsMaps && !isEmpty(filesResponse.sideEffectsMaps)
  if (!hasLocalSideEffects || opts.allowBuild?.(depNode.depPath) !== true) return remoteCacheKey
  const localCacheKey = calcDepState(opts.depGraph, opts.depsStateCache, depNode.dir, {
    includeDepGraphHash: shouldIncludeDepGraphHash({
      ignoreScripts: opts.ignoreScripts,
      deferDependencyBuilds: opts.deferDependencyBuilds,
      requiresBuild: depNode.requiresBuild,
    }),
    patchFileHash: depNode.patch?.hash,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: opts.nodeVersion,
  })
  return filesResponse.sideEffectsDiffs?.get(localCacheKey)?.remoteOrigin == null ? localCacheKey : remoteCacheKey
}

// For GVS packages that need building, add a .pnpm-needs-build marker to the
// filesMap. The import pipeline treats it as a normal file, so it gets
// written into the staging directory and atomically renamed with the rest
// of the package. On the next install, GVS fast paths detect the marker
// and force a re-fetch/re-import/re-build.
// Skip the marker when cached side effects will be applied (the package
// is already built and no build will run).
function addNeedsBuildMarker (
  filesResponse: PackageFilesResponse,
  { depNode, needsBuildMarkerSrc, sideEffectsCacheKey }: {
    depNode: DependenciesGraphNode
    needsBuildMarkerSrc: string | undefined
    sideEffectsCacheKey: SideEffectsCacheKey
  }
): PackageFilesResponse {
  if (needsBuildMarkerSrc == null) return filesResponse
  const hasCachedSideEffects = sideEffectsCacheKey != null &&
    filesResponse.sideEffectsMaps?.has(sideEffectsCacheKey) === true
  if (hasCachedSideEffects || !(depNode.requiresBuild || depNode.patch != null)) return filesResponse
  return {
    ...filesResponse,
    filesMap: new Map([...filesResponse.filesMap, ['.pnpm-needs-build', needsBuildMarkerSrc]]),
  }
}

async function importIntoSlot (
  depNode: DependenciesGraphNode,
  { filesResponse, sideEffectsCacheKey }: { filesResponse: PackageFilesResponse, sideEffectsCacheKey: SideEffectsCacheKey },
  { opts, storeController }: PackageImportContext
): Promise<ImportPackageResult | undefined> {
  // The marker is also there while another install builds the slot,
  // which a re-import would overwrite.
  const slotMarker = path.join(depNode.dir, '.pnpm-needs-build')
  const slotLock = opts.enableGlobalVirtualStore && await pathExists(slotMarker)
    ? await lockGlobalVirtualStoreSlot(depNode.modules)
    : undefined
  try {
    if (slotLock != null && !await pathExists(slotMarker)) {
      depNode.isBuilt = true
      return undefined
    }
    return await storeController.importPackage(depNode.dir, {
      filesResponse,
      force: depNode.forceImportPackage ?? opts.force,
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      requiresBuild: depNode.patch != null || depNode.requiresBuild,
      safeToSkip: opts.enableGlobalVirtualStore,
      sideEffectsCacheKey,
    })
  } finally {
    await slotLock?.release()
  }
}

async function linkSelfDependency (depNode: DependenciesGraphNode, depGraph: DependenciesGraph): Promise<void> {
  const selfDep = depNode.children[depNode.name]
  if (!selfDep) return
  const pkg = depGraph[selfDep]
  if (!pkg) return
  const targetModulesDir = path.join(depNode.modules, depNode.name, 'node_modules')
  await limitLinking(async () => symlinkDependency(pkg.dir, targetModulesDir, depNode.name))
}
