import path from 'node:path'

import { createStagingToken, selectImportDir } from '@pnpm/building.during-install'
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
  supportedArchitectures?: SupportedArchitectures
}

interface PackageImportContext {
  /** Names this install's global virtual store staging directories. */
  stagingToken?: string
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
  const importContext: PackageImportContext = {
    opts,
    restorer,
    stagingToken: opts.enableGlobalVirtualStore ? createStagingToken() : undefined,
    storeController,
  }
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
  const imported = await importIntoSlot(depNode, { filesResponse, sideEffectsCacheKey }, importContext)
  if (imported?.importMethod) {
    reportPackageImported({
      method: imported.importMethod,
      requester: importContext.opts.lockfileDir,
      to: imported.dir,
    })
  }
  depNode.isBuilt = imported?.isBuilt ?? true
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

/** Resolves to `undefined` when the global virtual store slot already holds the package. See `selectImportDir`. */
async function importIntoSlot (
  depNode: DependenciesGraphNode,
  { filesResponse, sideEffectsCacheKey }: { filesResponse: PackageFilesResponse, sideEffectsCacheKey: SideEffectsCacheKey },
  { opts, stagingToken, storeController }: PackageImportContext
): Promise<(ImportPackageResult & { dir: string }) | undefined> {
  const hasCachedBuild = sideEffectsCacheKey != null && filesResponse.sideEffectsMaps?.has(sideEffectsCacheKey) === true
  const importDir = await selectImportDir(depNode, { hasCachedBuild, opts: { ...opts, stagingToken } })
  if (importDir == null) return undefined
  if (importDir !== depNode.dir) {
    depNode.stagingDir = importDir
    return {
      ...await storeController.importPackage(importDir, {
        disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
        filesResponse,
        force: true,
        requiresBuild: true,
      }),
      dir: importDir,
    }
  }
  return {
    ...await storeController.importPackage(depNode.dir, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesResponse,
      force: depNode.forceImportPackage || opts.force,
      requiresBuild: depNode.patch != null || depNode.requiresBuild,
      safeToSkip: opts.enableGlobalVirtualStore,
      sideEffectsCacheKey,
    }),
    dir: depNode.dir,
  }
}

async function linkSelfDependency (depNode: DependenciesGraphNode, depGraph: DependenciesGraph): Promise<void> {
  const selfDep = depNode.children[depNode.name]
  if (!selfDep) return
  const pkg = depGraph[selfDep]
  if (!pkg) return
  const targetModulesDir = path.join(depNode.stagingDir ?? path.join(depNode.modules, depNode.name), 'node_modules')
  await limitLinking(async () => symlinkDependency(pkg.dir, targetModulesDir, depNode.name))
}
