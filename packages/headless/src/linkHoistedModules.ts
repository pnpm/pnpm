import {
  progressLogger,
  removalLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import difference from 'ramda/src/difference'
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
    force: boolean
    lockfileDir: string
    targetEngine?: string
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
    linkAllPkgsInOrder(storeController, graph, prevGraph, hierarchy, opts),
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
  opts: {
    force: boolean
    lockfileDir: string
    targetEngine?: string
  }
) {
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

      const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
        filesResponse,
        force: opts.force || depNode.depPath !== prevGraph[dir]?.depPath,
        targetEngine: opts.targetEngine,
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
      return linkAllPkgsInOrder(storeController, graph, prevGraph, deps, opts)
    })
  )
}
