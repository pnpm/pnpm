import { progressLogger } from '@pnpm/core-loggers'
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
  const dirsToRemove = difference(
    Object.keys(prevGraph),
    Object.keys(graph)
  )
  await Promise.all([
    ...dirsToRemove.map((dir) => rimraf(dir)),
    linkAllPkgsInOrder(storeController, graph, hierarchy, opts),
  ])
}

async function linkAllPkgsInOrder (
  storeController: StoreController,
  graph: DependenciesGraph,
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
        force: opts.force,
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
      return linkAllPkgsInOrder(storeController, graph, deps, opts)
    })
  )
}
