import path from 'node:path'

import {
  calcGraphNodeHash,
  type DepsGraph,
  type DepsStateCache,
  findRuntimeNodeVersion,
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
  // Resolve the project's pinned runtime Node version once per
  // invocation — the result drives every snapshot's GVS hash (or
  // the side-effects-cache key prefix in the non-GVS runtime
  // branch). `undefined` when no `engines.runtime` / `devEngines.runtime`
  // pin reached the lockfile, in which case the hasher falls through
  // to the host-detected Node.
  const nodeVersion = findRuntimeNodeVersion(Object.keys(lockfile.packages ?? {}))
  if (opts.enableGlobalVirtualStore) {
    for (const { hash, pkgMeta } of hashDependencyPaths(lockfile, {
      allowBuild: opts.allowBuild,
      supportedArchitectures: opts.supportedArchitectures,
      nodeVersion,
    })) {
      yield {
        dirInVirtualStore: path.join(opts.globalVirtualStoreDir, hash),
        pkgMeta,
      }
    }
  } else if (lockfile.packages) {
    let graphNodeHashOpts: { graph: DepsGraph<DepPath>, cache: DepsStateCache, supportedArchitectures?: SupportedArchitectures, nodeVersion?: string } | undefined
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
          nodeVersion,
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
  {
    allowBuild,
    supportedArchitectures,
    nodeVersion,
  }: {
    allowBuild?: AllowBuild
    supportedArchitectures?: SupportedArchitectures
    nodeVersion?: string
  }
): IterableIterator<HashedDepPath<PkgMetaAndSnapshot>> {
  const graph = lockfileToDepGraph(lockfile, supportedArchitectures)
  return iterateHashedGraphNodes(graph, iteratePkgMeta(lockfile, graph), allowBuild, supportedArchitectures, nodeVersion)
}
