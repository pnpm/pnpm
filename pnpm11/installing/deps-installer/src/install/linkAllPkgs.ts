import path from 'node:path'

import { reportPackageImported } from '@pnpm/core-loggers'
import { calcDepState, type DepsStateCache, shouldIncludeDepGraphHash } from '@pnpm/deps.graph-hasher'
import { symlinkDependency } from '@pnpm/fs.symlink-dependency'
import type {
  DependenciesGraph,
  DependenciesGraphNode,
} from '@pnpm/installing.deps-resolver'
import { logger } from '@pnpm/logger'
import { createRemoteSideEffectsRestorer, type RemoteSideEffectsRestorer } from '@pnpm/pnpr.client'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  DepPath,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import pLimit from 'p-limit'
import { isEmpty } from 'ramda'

const limitLinking = pLimit(16)

export interface LinkAllPkgsOptions {
  allowBuild?: AllowBuild
  depGraph: DependenciesGraph
  depsStateCache: DepsStateCache
  deferDependencyBuilds: boolean
  disableRelinkLocalDirDeps?: boolean
  enableGlobalVirtualStore: boolean
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

interface ImportContext {
  opts: LinkAllPkgsOptions
  restorer: RemoteSideEffectsRestorer<DepPath> | undefined
  storeController: StoreController
}

type FetchedFiles = Awaited<ReturnType<DependenciesGraphNode['fetching']>>['files']

export async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: LinkAllPkgsOptions
): Promise<void> {
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
  await Promise.all(
    depNodes.map(async (depNode) => importDepNode(depNode, { opts, restorer, storeController }))
  )
}

async function importDepNode (depNode: DependenciesGraphNode, context: ImportContext): Promise<void> {
  const { opts, storeController } = context
  const { files } = await depNode.fetching()
  depNode.requiresBuild = files.requiresBuild
  const sideEffectsCacheKey = await getSideEffectsCacheKey(depNode, files, context)
  const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
    disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
    filesResponse: files,
    force: opts.force,
    safeToSkip: opts.enableGlobalVirtualStore,
    sideEffectsCacheKey,
    requiresBuild: depNode.patch != null || depNode.requiresBuild,
  })
  if (importMethod) {
    reportPackageImported({
      method: importMethod,
      requester: opts.lockfileDir,
      to: depNode.dir,
    })
  }
  depNode.isBuilt = isBuilt

  await linkSelfDependency(depNode, opts.depGraph)
}

/**
 * The side-effects cache entry to import the package with: the one the remote
 * cache restored, or else a locally cached build this package is allowed to use.
 */
async function getSideEffectsCacheKey (
  depNode: DependenciesGraphNode,
  files: FetchedFiles,
  { opts, restorer }: ImportContext
): Promise<string | undefined> {
  const remoteCacheKey = await restorer?.restore({
    graphKey: depNode.depPath,
    depPath: depNode.depPath,
    files,
    filesIndexFile: depNode.filesIndexFile,
    name: depNode.name,
    patchFileHash: depNode.patch?.hash,
    resolution: depNode.resolution,
    version: depNode.version,
  })
  if (remoteCacheKey != null) return remoteCacheKey
  if (!opts.sideEffectsCacheRead || !files.sideEffectsMaps || isEmpty(files.sideEffectsMaps)) return remoteCacheKey
  if (opts.allowBuild?.(depNode.depPath) !== true) return remoteCacheKey
  const localCacheKey = calcDepState(opts.depGraph, opts.depsStateCache, depNode.depPath, {
    includeDepGraphHash: shouldIncludeDepGraphHash({
      ignoreScripts: opts.ignoreScripts,
      deferDependencyBuilds: opts.deferDependencyBuilds,
      requiresBuild: depNode.requiresBuild,
    }),
    patchFileHash: depNode.patch?.hash,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: opts.nodeVersion,
  })
  return files.sideEffectsDiffs?.get(localCacheKey)?.remoteOrigin == null ? localCacheKey : remoteCacheKey
}

async function linkSelfDependency (depNode: DependenciesGraphNode, depGraph: DependenciesGraph): Promise<void> {
  const selfDep = depNode.children[depNode.name]
  if (!selfDep) return
  const pkg = depGraph[selfDep]
  if (!pkg || !pkg.installable && pkg.optional) return
  const targetModulesDir = path.join(depNode.modules, depNode.name, 'node_modules')
  await limitLinking(async () => symlinkDependency(pkg.dir, targetModulesDir, depNode.name))
}
