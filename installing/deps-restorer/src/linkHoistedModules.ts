import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import {
  progressLogger,
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import type {
  DependenciesGraph,
  DepHierarchy,
} from '@pnpm/deps.graph-builder'
import { calcDepState, type DepsStateCache } from '@pnpm/deps.graph-hasher'
import { logger } from '@pnpm/logger'
import type {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store.controller-types'
import type { AllowBuild, SupportedArchitectures } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import pLimit from 'p-limit'
import { difference, isEmpty } from 'ramda'

const limitLinking = pLimit(16)

export async function linkHoistedModules (
  storeController: StoreController,
  graph: DependenciesGraph,
  prevGraph: DependenciesGraph,
  hierarchy: DepHierarchy,
  opts: {
    allowBuild?: AllowBuild
    depsStateCache: DepsStateCache
    disableRelinkLocalDirDeps?: boolean
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    preferSymlinkedExecutables?: boolean
    sideEffectsCacheRead: boolean
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<void> {
  // TODO: remove nested node modules first
  const dirsToRemove = difference(
    Object.keys(prevGraph),
    Object.keys(graph)
  )
  statsLogger.debug({
    prefix: opts.lockfileDir,
    removed: dirsToRemove.length,
  })
  // We should avoid removing unnecessary directories while simultaneously adding new ones.
  // Doing so can sometimes lead to a race condition when linking commands to `node_modules/.bin`.
  await Promise.all(dirsToRemove.map((dir) => tryRemoveDir(dir)))
  await Promise.all(
    Object.entries(hierarchy)
      .map(([parentDir, depsHierarchy]) => {
        function warn (message: string) {
          logger.info({
            message,
            prefix: parentDir,
          })
        }
        return linkAllPkgsInOrder(storeController, graph, depsHierarchy, parentDir, {
          ...opts,
          warn,
        })
      })
  )
}

async function tryRemoveDir (dir: string): Promise<void> {
  removalLogger.debug(dir)
  try {
    await rimraf(dir)
  } catch (err: any) { // eslint-disable-line
    /* Just ignoring for now. Not even logging.
    logger.warn({
      error: err,
      message: `Failed to remove "${pathToRemove}"`,
      prefix: lockfileDir,
    })
    */
  }
}

async function linkAllPkgsInOrder (
  storeController: StoreController,
  graph: DependenciesGraph,
  hierarchy: DepHierarchy,
  parentDir: string,
  opts: {
    allowBuild?: AllowBuild
    depsStateCache: DepsStateCache
    disableRelinkLocalDirDeps?: boolean
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    preferSymlinkedExecutables?: boolean
    sideEffectsCacheRead: boolean
    supportedArchitectures?: SupportedArchitectures
    warn: (message: string) => void
  }
): Promise<void> {
  await Promise.all(
    Object.entries(hierarchy).map(async ([dir, deps]) => {
      const depNode = graph[dir]
      if (depNode.fetching) {
        let filesResponse!: PackageFilesResponse
        try {
          filesResponse = (await depNode.fetching()).files
        } catch (err: any) { // eslint-disable-line
          if (depNode.optional) return
          throw err
        }

        depNode.requiresBuild = filesResponse.requiresBuild
        let sideEffectsCacheKey: string | undefined
        if (opts.sideEffectsCacheRead && filesResponse.sideEffectsMaps && !isEmpty(filesResponse.sideEffectsMaps)) {
          if (opts.allowBuild?.(depNode.name, depNode.version) === true) {
            sideEffectsCacheKey = calcDepState(graph, opts.depsStateCache, dir, {
              includeDepGraphHash: !opts.ignoreScripts && depNode.requiresBuild, // true when is built
              patchFileHash: depNode.patch?.hash,
              supportedArchitectures: opts.supportedArchitectures,
            })
          }
        }
        // Limiting the concurrency here fixes an out of memory error.
        // It is not clear why it helps as importing is also limited inside fs.indexed-pkg-importer.
        // The out of memory error was reproduced on the teambit/bit repository with the "rootComponents" feature turned on
        await limitLinking(async () => {
          const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
            filesResponse,
            force: true,
            disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
            keepModulesDir: true,
            requiresBuild: depNode.patch != null || depNode.requiresBuild,
            sideEffectsCacheKey,
          })
          if (importMethod) {
            progressLogger.debug({
              method: importMethod,
              requester: opts.lockfileDir,
              status: 'imported',
              to: depNode.dir,
            })
          }
          depNode.isBuilt = isBuilt
        })
      }
      return linkAllPkgsInOrder(storeController, graph, deps, dir, opts)
    })
  )
  const modulesDir = path.join(parentDir, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  await linkBins(modulesDir, binsDir, {
    allowExoticManifests: true,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    warn: opts.warn,
  })
}
