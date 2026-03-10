import path from 'path'
import {
  iterateHashedGraphNodes,
  iteratePkgMeta,
  lockfileToDepGraph,
  calcGraphNodeHash,
  type PkgMetaAndSnapshot,
  type DepsGraph,
  type HashedDepPath,
  type DepsStateCache,
} from '@pnpm/calc-dep-state'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import type { AllowBuild, DepPath } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'

interface PkgSnapshotWithLocation {
  pkgMeta: PkgMetaAndSnapshot
  dirInVirtualStore: string
}

export function * iteratePkgsForVirtualStore (lockfile: LockfileObject, opts: {
  allowBuild?: AllowBuild
  enableGlobalVirtualStore?: boolean
  virtualStoreDirMaxLength: number
  virtualStoreDir: string
  globalVirtualStoreDir: string
}): IterableIterator<PkgSnapshotWithLocation> {
  if (opts.enableGlobalVirtualStore) {
    for (const { hash, pkgMeta } of hashDependencyPaths(lockfile, opts.allowBuild)) {
      yield {
        dirInVirtualStore: path.join(opts.globalVirtualStoreDir, hash),
        pkgMeta,
      }
    }
  } else if (lockfile.packages) {
    let graphNodeHashOpts: { graph: DepsGraph<DepPath>, cache: DepsStateCache } | undefined
    for (const depPath in lockfile.packages) {
      if (!Object.hasOwn(lockfile.packages, depPath)) {
        continue
      }
      const pkgSnapshot = lockfile.packages[depPath as DepPath]
      const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const pkgMeta = {
        depPath: depPath as DepPath,
        pkgIdWithPatchHash: dp.getPkgIdWithPatchHash(depPath as DepPath),
        name,
        version,
        pkgSnapshot,
      }
      let dirInVirtualStore!: string
      if (dp.isRuntimeDepPath(depPath as DepPath)) {
        graphNodeHashOpts ??= {
          cache: {},
          graph: lockfileToDepGraph(lockfile),
        }
        const hash = calcGraphNodeHash(graphNodeHashOpts, pkgMeta)
        dirInVirtualStore = path.join(opts.globalVirtualStoreDir, hash)
      } else {
        dirInVirtualStore = path.join(opts.virtualStoreDir, dp.depPathToFilename(depPath, opts.virtualStoreDirMaxLength))
      }
      yield {
        dirInVirtualStore,
        pkgMeta,
      }
    }
  }
}

function hashDependencyPaths (lockfile: LockfileObject, allowBuild?: AllowBuild): IterableIterator<HashedDepPath<PkgMetaAndSnapshot>> {
  const graph = lockfileToDepGraph(lockfile)
  return iterateHashedGraphNodes(graph, iteratePkgMeta(lockfile, graph), allowBuild)
}
