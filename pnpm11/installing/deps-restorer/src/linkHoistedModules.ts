import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

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
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { logger } from '@pnpm/logger'
import type {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store.controller-types'
import type { AllowBuild, SupportedArchitectures } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import pLimit from 'p-limit'
import { pathExists } from 'path-exists'
import { difference, isEmpty } from 'ramda'
import { renameOverwrite } from 'rename-overwrite'

const limitLinking = pLimit(16)

/** A package directory on disk that the hoisting plan does not place. */
interface UnplannedDir {
  dir: string
  modulesDir: string
  /** `dir` relative to `modulesDir`, so scoped packages keep their `@scope/` prefix. */
  pkgName: string
}

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
  // A directory the previous install recorded is pnpm's to delete. One that only
  // the on-disk scan found is not: pnpm has no record of putting it there, so it
  // is quarantined instead, the way an alien package directory already is.
  const recordedDirs = new Set(dirsToRemove)
  const dirsToQuarantine = (await findUnplannedDirs(hierarchy))
    .filter(({ dir }) => !recordedDirs.has(dir))
  statsLogger.debug({
    prefix: opts.lockfileDir,
    removed: dirsToRemove.length + dirsToQuarantine.length,
  })
  // We should avoid removing unnecessary directories while simultaneously adding new ones.
  // Doing so can sometimes lead to a race condition when linking commands to `node_modules/.bin`.
  await Promise.all([
    ...dirsToRemove.map((dir) => tryRemoveDir(dir)),
    ...dirsToQuarantine.map((unplanned) => quarantineDir(unplanned, opts.lockfileDir)),
  ])
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

/**
 * Move a package directory pnpm has no record of installing into the
 * `node_modules/.ignored` sibling of wherever it sits.
 *
 * Deleting is reserved for what the previous install recorded placing. Anything
 * else may hold work someone did by hand, so it is displaced rather than
 * destroyed — the treatment `safeIsInnerLink` already gives a package directory
 * that pnpm did not put there. Getting it out of `node_modules` is what makes
 * the tree correct; the bytes are incidental.
 */
async function quarantineDir ({ dir, modulesDir, pkgName }: UnplannedDir, lockfileDir: string): Promise<void> {
  const ignoredDir = path.join(modulesDir, '.ignored', pkgName)
  removalLogger.debug(dir)
  try {
    await fs.mkdir(path.dirname(ignoredDir), { recursive: true })
    await renameOverwrite(dir, ignoredDir)
  } catch (err: unknown) {
    logger.warn({
      error: err as Error,
      message: `Failed to move "${dir}" to "${ignoredDir}"`,
      prefix: lockfileDir,
    })
    return
  }
  logger.warn({
    message: `Moving ${pkgName} to "node_modules/.ignored". It is not in the dependency tree and pnpm has no record of installing it.`,
    prefix: path.dirname(modulesDir),
  })
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

/**
 * Package directories that physically exist inside the projects' `node_modules`
 * but that the new hoisting plan does not place there.
 *
 * The `prevGraph`/`graph` difference cannot see them: an install that is
 * interrupted before the current lockfile and `.modules.yaml` are written
 * leaves nested copies on disk while the next install starts from an empty
 * `prevGraph`, so nothing ever reclaims them
 * (https://github.com/pnpm/pnpm/issues/13676).
 *
 * Symlinks are skipped: that is how workspace packages and `link:` dependencies
 * are attached, and they are absent from the graph by design.
 */
async function findUnplannedDirs (hierarchy: DepHierarchy): Promise<UnplannedDir[]> {
  const unplannedDirs = await Promise.all(
    Object.entries(hierarchy).map(async ([projectDir, plannedDeps]) => {
      const modulesDir = path.join(projectDir, 'node_modules')
      const pkgNames = await readModulesDir(modulesDir)
      if (pkgNames == null) return []
      const plannedDirs = new Set(Object.keys(plannedDeps))
      const candidates = pkgNames
        .map((pkgName) => ({ dir: path.join(modulesDir, pkgName), modulesDir, pkgName }))
        .filter(({ dir }) => !plannedDirs.has(dir))
      const checked = await Promise.all(
        candidates.map(async (candidate) => await isPackageDir(candidate.dir) ? candidate : null)
      )
      return checked.filter((candidate) => candidate != null)
    })
  )
  return unplannedDirs.flat()
}

/**
 * Whether `dir` holds a package rather than something else that happens to sit
 * in a `node_modules`.
 *
 * `package.json` is the marker [`importIndexedDir`] writes last, so a directory
 * carrying one is a package that was materialized in full. A directory without
 * one is not a package, and pruning is not entitled to remove it however little
 * the hoisting plan has to say about it.
 */
async function isPackageDir (dir: string): Promise<boolean> {
  if (!await isRealDir(dir)) return false
  return pathExists(path.join(dir, 'package.json'))
}

async function isRealDir (dir: string): Promise<boolean> {
  try {
    return (await fs.lstat(dir)).isDirectory()
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}
