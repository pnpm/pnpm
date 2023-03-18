import path from 'path'
import { calcDepState, type DepsStateCache } from '@pnpm/calc-dep-state'
import {
  progressLogger,
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import { linkBins } from '@pnpm/link-bins'
import { logger } from '@pnpm/logger'
import {
  type PackageFilesResponse,
  type StoreController,
} from '@pnpm/store-controller-types'
import pLimit from 'p-limit'
import difference from 'ramda/src/difference'
import isEmpty from 'ramda/src/isEmpty'
import rimraf from '@zkochan/rimraf'
import {
  type DepHierarchy,
  type DependenciesGraph,
} from './lockfileToDepGraph'

const limitLinking = pLimit(16)

export async function linkHoistedModules (
  storeController: StoreController,
  graph: DependenciesGraph,
  prevGraph: DependenciesGraph,
  hierarchy: DepHierarchy,
  opts: {
    depsStateCache: DepsStateCache
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    preferSymlinkedExecutables?: boolean
    sideEffectsCacheRead: boolean
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
  await Promise.all([
    ...dirsToRemove.map((dir) => tryRemoveDir(dir)),
    ...Object.entries(hierarchy)
      .map(([parentDir, depsHierarchy]) => {
        function warn (message: string) {
          logger.info({
            message,
            prefix: parentDir,
          })
        }
        return linkAllPkgsInOrder(storeController, graph, prevGraph, depsHierarchy, parentDir, {
          ...opts,
          warn,
        })
      }),
  ])
}

async function tryRemoveDir (dir: string) {
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
  prevGraph: DependenciesGraph,
  hierarchy: DepHierarchy,
  parentDir: string,
  opts: {
    depsStateCache: DepsStateCache
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    preferSymlinkedExecutables?: boolean
    sideEffectsCacheRead: boolean
    warn: (message: string) => void
  }
) {
  const _calcDepState = calcDepState.bind(null, graph, opts.depsStateCache)
  await Promise.all(
    Object.entries(hierarchy).map(async ([dir, deps]) => {
      const depNode = graph[dir]
      if (depNode.fetchingFiles) {
        let filesResponse!: PackageFilesResponse
        try {
          filesResponse = await depNode.fetchingFiles()
        } catch (err: any) { // eslint-disable-line
          if (depNode.optional) return
          throw err
        }

        let sideEffectsCacheKey: string | undefined
        if (opts.sideEffectsCacheRead && filesResponse.sideEffects && !isEmpty(filesResponse.sideEffects)) {
          sideEffectsCacheKey = _calcDepState(dir, {
            isBuilt: !opts.ignoreScripts && depNode.requiresBuild,
            patchFileHash: depNode.patchFile?.hash,
          })
        }
        // Limiting the concurrency here fixes an out of memory error.
        // It is not clear why it helps as importing is also limited inside fs.indexed-pkg-importer.
        // The out of memory error was reproduced on the teambit/bit repository with the "rootComponents" feature turned on
        await limitLinking(async () => {
          const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
            filesResponse,
            force: opts.force || depNode.depPath !== prevGraph[dir]?.depPath,
            keepModulesDir: true,
            requiresBuild: depNode.requiresBuild || depNode.patchFile != null,
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
      return linkAllPkgsInOrder(storeController, graph, prevGraph, deps, dir, opts)
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
