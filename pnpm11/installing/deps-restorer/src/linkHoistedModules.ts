import { promises as fs } from 'node:fs'
import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import {
  removalLogger,
  reportPackageImported,
  statsLogger,
} from '@pnpm/core-loggers'
import type {
  DependenciesGraph,
  DepHierarchy,
} from '@pnpm/deps.graph-builder'
import { calcDepState, type DepsStateCache, findRuntimeNodeVersion } from '@pnpm/deps.graph-hasher'
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
  const physicalDirsList = await Promise.all(
    Object.keys(hierarchy).map((parentDir) => findPhysicalDirs(path.join(parentDir, 'node_modules')))
  )
  const physicalDirs = physicalDirsList.flat()
  const orphanPhysicalDirs = physicalDirs.filter((dir) => !graph[dir])

  const dirsToRemove = Array.from(new Set([
    ...difference(
      Object.keys(prevGraph),
      Object.keys(graph)
    ),
    ...orphanPhysicalDirs,
  ]))
  statsLogger.debug({
    prefix: opts.lockfileDir,
    removed: dirsToRemove.length,
  })
  // We should avoid removing unnecessary directories while simultaneously adding new ones.
  // Doing so can sometimes lead to a race condition when linking commands to `node_modules/.bin`.
  await Promise.all(dirsToRemove.map((dir) => tryRemoveDir(dir)))
  // Resolve the project's pinned runtime Node version once, before
  // the recursive walk. The graph is keyed by install directory in
  // this module, so scanning `Object.keys(graph)` would miss every
  // `node@runtime:<version>` entry — pull the depPath off each
  // node instead. Threading it down via `opts` also avoids a
  // re-scan at every recursion level.
  const nodeVersion = findRuntimeNodeVersion(
    Object.values(graph).map((node) => node.depPath)
  )
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
          nodeVersion,
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
    /**
     * Resolved `engines.runtime` Node version, computed once by
     * [`linkHoistedModules`] before the recursion. Threaded into
     * each [`calcDepState`] call so the side-effects-cache key
     * prefix tracks the script-runner Node rather than pnpm's own
     * `process.version`.
     */
    nodeVersion?: string
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
          if (opts.allowBuild?.(depNode.depPath) === true) {
            sideEffectsCacheKey = calcDepState(graph, opts.depsStateCache, dir, {
              includeDepGraphHash: !opts.ignoreScripts && depNode.requiresBuild, // true when is built
              patchFileHash: depNode.patch?.hash,
              supportedArchitectures: opts.supportedArchitectures,
              nodeVersion: opts.nodeVersion,
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
            reportPackageImported({
              method: importMethod,
              requester: opts.lockfileDir,
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

async function findPhysicalDirs (modulesDir: string): Promise<string[]> {
  const dirs: string[] = []
  let files: string[] = []
  try {
    files = await fs.readdir(modulesDir)
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return []
    throw err
  }
  await Promise.all(
    files.map(async (file) => {
      if (file === '.bin' || file === '.ignored' || file === '.pnpm') return
      const filePath = path.join(modulesDir, file)
      let stat
      try {
        stat = await fs.lstat(filePath)
      } catch {
        return
      }
      if (stat.isSymbolicLink()) return
      if (!stat.isDirectory()) return

      if (file.startsWith('@')) {
        let subFiles: string[] = []
        try {
          subFiles = await fs.readdir(filePath)
        } catch {
          return
        }
        await Promise.all(
          subFiles.map(async (subFile) => {
            const subFilePath = path.join(filePath, subFile)
            let subStat
            try {
              subStat = await fs.lstat(subFilePath)
            } catch {
              return
            }
            if (subStat.isSymbolicLink()) return
            if (subStat.isDirectory()) {
              dirs.push(subFilePath)
            }
          })
        )
      } else {
        dirs.push(filePath)
      }
    })
  )
  return dirs
}
