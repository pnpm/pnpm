import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import * as dp from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import {
  nameVerFromPkgSnapshot,
  packageIsIndependent,
  type PackageSnapshots,
} from '@pnpm/lockfile.utils'
import { lockfileWalker, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { logger } from '@pnpm/logger'
import type { DepPath } from '@pnpm/types'
import pLimit from 'p-limit'

import { builtBinPackages, builtProjectBinPackages } from './builtBinPackages.js'
import type { StrictBuildOptions } from './extendBuildOptions.js'
import type { RebuildPackagesContext, RebuildState } from './rebuildTypes.js'

const limitLinking = pLimit(16)

/**
 * The packages to rebuild and every package depending on one of them, reachable from the
 * projects, each mapped to its dependencies within that set.
 */
export function getGraphToBuild (ctx: RebuildPackagesContext, opts: StrictBuildOptions): Map<DepPath, DepPath[]> {
  const pkgSnapshots: PackageSnapshots = ctx.currentLockfile.packages ?? {}
  const nodesToBuildAndTransitive = new Set<DepPath>()
  getSubgraphToBuild(
    lockfileWalker(
      ctx.currentLockfile,
      Object.values(ctx.projects).map(({ id }) => id),
      {
        include: {
          dependencies: opts.production,
          devDependencies: opts.development,
          optionalDependencies: opts.optional,
        },
        resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
      }
    ).step,
    nodesToBuildAndTransitive,
    { pkgsToRebuild: ctx.pkgsToRebuild }
  )
  const graph = new Map<DepPath, DepPath[]>()
  for (const depPath of Array.from(nodesToBuildAndTransitive)) {
    const pkgSnapshot = pkgSnapshots[depPath]
    graph.set(depPath, Object.entries({ ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((childRelDepPath): childRelDepPath is DepPath => childRelDepPath != null && nodesToBuildAndTransitive.has(childRelDepPath)))
  }
  return graph
}

function getSubgraphToBuild (
  step: LockfileWalkerStep,
  nodesToBuildAndTransitive: Set<DepPath>,
  opts: {
    pkgsToRebuild: Set<string>
  }
): boolean {
  let currentShouldBeBuilt = false
  for (const { depPath, next } of step.dependencies) {
    if (nodesToBuildAndTransitive.has(depPath)) {
      currentShouldBeBuilt = true
    }

    const childShouldBeBuilt = getSubgraphToBuild(next(), nodesToBuildAndTransitive, opts) ||
      opts.pkgsToRebuild.has(depPath)
    if (childShouldBeBuilt) {
      nodesToBuildAndTransitive.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  for (const depPath of step.missing) {
    logger.debug({ message: `No entry for "${depPath}" in pnpm-lock.yaml` })
  }
  return currentShouldBeBuilt
}

export async function relinkBins (state: RebuildState, getPkgModulesDir: (depPath: DepPath, state: RebuildState) => string): Promise<void> {
  const { pkgSnapshots, warn } = state
  await Promise.all(
    (Object
      .keys(pkgSnapshots) as DepPath[])
      .filter((depPath) => !packageIsIndependent(pkgSnapshots[depPath]))
      .map(async (depPath) => limitLinking(async () => {
        const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshots[depPath])
        const modules = getPkgModulesDir(depPath, state)
        const binPath = path.join(safeJoinModulesDir(modules, pkgInfo.name), 'node_modules', '.bin')
        const snapshot = pkgSnapshots[depPath]
        const forceForPackages = builtBinPackages(modules, { ...snapshot.dependencies, ...snapshot.optionalDependencies }, state)
        return linkBins(modules, binPath, { warn, forceForPackages })
      }))
  )
  await Promise.all(Object.values(state.ctx.projects).map(async ({ id, rootDir }) => limitLinking(async () => {
    const modules = path.join(rootDir, 'node_modules')
    const binPath = path.join(modules, '.bin')
    const importer = state.ctx.currentLockfile.importers[id]
    const dependencies = { ...importer?.dependencies, ...importer?.devDependencies, ...importer?.optionalDependencies }
    return linkBins(modules, binPath, {
      allowExoticManifests: true,
      forceForPackages: builtProjectBinPackages(modules, dependencies, state),
      warn,
    })
  })))
}

export function binDirsInAllParentDirs (pkgRoot: string, lockfileDir: string): string[] {
  const binDirs: string[] = []
  let dir = pkgRoot
  do {
    if (!(path.dirname(dir)[0] === '@')) {
      binDirs.push(path.join(dir, 'node_modules/.bin'))
    }
    dir = path.dirname(dir)
  } while (path.relative(dir, lockfileDir) !== '')
  binDirs.push(path.join(lockfileDir, 'node_modules/.bin'))
  return binDirs
}
