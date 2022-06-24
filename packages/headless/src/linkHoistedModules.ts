import path from 'path'
import { calcDepState, DepsStateCache } from '@pnpm/calc-dep-state'
import {
  progressLogger,
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import linkBins from '@pnpm/link-bins'
import logger from '@pnpm/logger'
import {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import difference from 'ramda/src/difference'
import isEmpty from 'ramda/src/isEmpty'
import rimraf from '@zkochan/rimraf'
import {
  DepHierarchy,
  DependenciesGraph,
} from './lockfileToDepGraph'

export default async function linkHoistedModules (
  storeController: StoreController,
  graph: DependenciesGraph,
  prevGraph: DependenciesGraph,
  hierarchy: DepHierarchy,
  opts: {
    depsStateCache: DepsStateCache
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
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
    sideEffectsCacheRead: boolean
    warn: (message: string) => void
  }
) {
  const _calcDepState = calcDepState.bind(null, graph, opts.depsStateCache)
  await Promise.all(
    Object.entries(hierarchy).map(async ([dir, deps]) => {
      const depNode = graph[dir]
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
          patchFileHash: depNode.patchFile?.hash,
          ignoreScripts: opts.ignoreScripts,
        })
      }
      const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
        filesResponse,
        force: opts.force || depNode.depPath !== prevGraph[dir]?.depPath,
        requiresBuild: depNode.requiresBuild,
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
      return linkAllPkgsInOrder(storeController, graph, prevGraph, deps, dir, opts)
    })
  )
  const modulesDir = path.join(parentDir, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  await linkBins(modulesDir, binsDir, {
    allowExoticManifests: true,
    warn: opts.warn,
  })
}
