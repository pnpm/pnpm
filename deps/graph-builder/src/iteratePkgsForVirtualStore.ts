import path from 'node:path'

import {
  calcGraphNodeHash,
  type DepsGraph,
  type DepsStateCache,
  type HashedDepPath,
  iterateHashedGraphNodes,
  iteratePkgMeta,
  lockfileToDepGraph,
  type PkgMetaAndSnapshot,
} from '@pnpm/deps.graph-hasher'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import type { AllowBuild, DepPath, SupportedArchitectures } from '@pnpm/types'

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
  supportedArchitectures?: SupportedArchitectures
}): IterableIterator<PkgSnapshotWithLocation> {
  if (opts.enableGlobalVirtualStore) {
    for (const { hash, pkgMeta } of hashDependencyPaths(lockfile, opts.allowBuild, opts.supportedArchitectures)) {
      yield {
        dirInVirtualStore: path.join(opts.globalVirtualStoreDir, hash),
        pkgMeta,
      }
    }
  } else if (lockfile.packages) {
    let graphNodeHashOpts: { graph: DepsGraph<DepPath>, cache: DepsStateCache, supportedArchitectures?: SupportedArchitectures } | undefined
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
          graph: lockfileToDepGraph(lockfile, opts.supportedArchitectures),
          supportedArchitectures: opts.supportedArchitectures,
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

function hashDependencyPaths (
  lockfile: LockfileObject,
  allowBuild?: AllowBuild,
  supportedArchitectures?: SupportedArchitectures
): IterableIterator<HashedDepPath<PkgMetaAndSnapshot>> {
  const graph = lockfileToDepGraph(lockfile, supportedArchitectures)
  return iterateHashedGraphNodes(graph, iteratePkgMeta(lockfile, graph), allowBuild, supportedArchitectures)
}
