import { getPkgIdWithPatchHash } from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'

import type { DepsGraph, PkgMeta, PkgMetaIterator } from './types.js'

export interface PkgMetaAndSnapshot extends PkgMeta {
  pkgSnapshot: PackageSnapshot
  pkgIdWithPatchHash: PkgIdWithPatchHash
}

export function * iteratePkgMeta (lockfile: LockfileObject, graph: DepsGraph<DepPath>): PkgMetaIterator<PkgMetaAndSnapshot> {
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
      pkgIdWithPatchHash: graph[depPath as DepPath]?.pkgIdWithPatchHash ?? getPkgIdWithPatchHash(depPath as DepPath),
      pkgSnapshot,
    }
  }
}
