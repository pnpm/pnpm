import {
  iterateHashedGraphNodes,
  lockfileToDepGraph,
  type PkgMeta,
  type DepsGraph,
  type PkgMetaIterator,
  type HashedDepPath,
} from '@pnpm/calc-dep-state'
import { type LockfileObject, type PackageSnapshot } from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import { type DepPath, type PkgIdWithPatchHash } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'

interface PkgSnapshotWithLocation {
  pkgMeta: PkgMetaAndSnapshot
  dirNameInVirtualStore: string
}

export function * iteratePkgsForVirtualStore (lockfile: LockfileObject, opts: {
  enableGlobalVirtualStore?: boolean
  virtualStoreDirMaxLength: number
}): IterableIterator<PkgSnapshotWithLocation> {
  if (opts.enableGlobalVirtualStore) {
    for (const { hash, pkgMeta } of hashDependencyPaths(lockfile)) {
      yield {
        dirNameInVirtualStore: hash,
        pkgMeta,
      }
    }
  } else if (lockfile.packages) {
    for (const depPath in lockfile.packages) {
      if (Object.hasOwn(lockfile.packages, depPath)) {
        const pkgSnapshot = lockfile.packages[depPath as DepPath]
        const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        yield {
          pkgMeta: {
            depPath: depPath as DepPath,
            pkgIdWithPatchHash: dp.getPkgIdWithPatchHash(depPath as DepPath),
            name,
            version,
            pkgSnapshot,
          },
          dirNameInVirtualStore: dp.depPathToFilename(depPath, opts.virtualStoreDirMaxLength),
        }
      }
    }
  }
}

interface PkgMetaAndSnapshot extends PkgMeta {
  pkgSnapshot: PackageSnapshot
  pkgIdWithPatchHash: PkgIdWithPatchHash
}

function hashDependencyPaths (lockfile: LockfileObject): IterableIterator<HashedDepPath<PkgMetaAndSnapshot>> {
  const graph = lockfileToDepGraph(lockfile)
  return iterateHashedGraphNodes(graph, iteratePkgMeta(lockfile, graph))
}

function * iteratePkgMeta (lockfile: LockfileObject, graph: DepsGraph<DepPath>): PkgMetaIterator<PkgMetaAndSnapshot> {
  if (lockfile.packages == null) {
    return
  }
  for (const depPath in lockfile.packages) {
    if (!Object.hasOwn(lockfile.packages, depPath)) {
      continue
    }
    const pkgSnapshot = lockfile.packages[depPath as DepPath]
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    yield {
      name,
      version,
      depPath: depPath as DepPath,
      pkgIdWithPatchHash: graph[depPath as DepPath].pkgIdWithPatchHash ?? dp.getPkgIdWithPatchHash(depPath as DepPath),
      pkgSnapshot,
    }
  }
}
