import path from 'node:path'

import {
  calcGraphNodeHash,
  type DepsGraph,
  type DepsStateCache,
  type GraphNodeHashOptions,
  type HashedDepPath,
  iterateHashedGraphNodes,
  iteratePkgMeta,
  lockfileToDepGraph,
  type PkgMetaAndSnapshot,
} from '@pnpm/deps.graph-hasher'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import {
  findLockedRootNodeRuntime,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import type { AllowBuild, DepPath, SupportedArchitectures } from '@pnpm/types'

export interface PkgSnapshotWithLocation {
  pkgMeta: PkgMetaAndSnapshot
  dirInVirtualStore: string
}

interface IteratePkgsForVirtualStoreOptions {
  allowBuild?: AllowBuild
  enableGlobalVirtualStore?: boolean
  lockfileDir: string
  virtualStoreDirMaxLength: number
  virtualStoreDir: string
  globalVirtualStoreDir: string
  supportedArchitectures?: SupportedArchitectures
}

interface GraphNodeHashOpts {
  graph: DepsGraph<DepPath>
  cache: DepsStateCache
  supportedArchitectures?: SupportedArchitectures
  nodeVersion?: string
  lockfileDir: string
}

export function * iteratePkgsForVirtualStore (lockfile: LockfileObject, opts: IteratePkgsForVirtualStoreOptions): IterableIterator<PkgSnapshotWithLocation> {
  // Resolve the root project's pinned runtime Node version once per
  // invocation — the result drives every snapshot's GVS hash (or
  // the side-effects-cache key prefix in the non-GVS runtime
  // branch). `undefined` when no `engines.runtime` / `devEngines.runtime`
  // pin reached the lockfile, in which case the hasher falls through
  // to the host-detected Node.
  const nodeVersion = findLockedRootNodeRuntime(lockfile)?.version
  if (opts.enableGlobalVirtualStore) {
    yield * iteratePkgsInGlobalVirtualStore(lockfile, opts, nodeVersion)
  } else if (lockfile.packages) {
    yield * iteratePkgsInLocalVirtualStore(lockfile, opts, nodeVersion)
  }
}

function * iteratePkgsInGlobalVirtualStore (
  lockfile: LockfileObject,
  opts: IteratePkgsForVirtualStoreOptions,
  nodeVersion: string | undefined
): IterableIterator<PkgSnapshotWithLocation> {
  for (const { hash, pkgMeta } of hashDependencyPaths(lockfile, {
    allowBuild: opts.allowBuild,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion,
    lockfileDir: opts.lockfileDir,
  })) {
    yield {
      dirInVirtualStore: path.join(opts.globalVirtualStoreDir, hash),
      pkgMeta,
    }
  }
}

function * iteratePkgsInLocalVirtualStore (
  lockfile: LockfileObject,
  opts: IteratePkgsForVirtualStoreOptions,
  nodeVersion: string | undefined
): IterableIterator<PkgSnapshotWithLocation> {
  let graphNodeHashOpts: GraphNodeHashOpts | undefined
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
    if (!dp.isRuntimeDepPath(depPath as DepPath)) {
      yield {
        dirInVirtualStore: path.join(opts.virtualStoreDir, dp.depPathToFilename(depPath, opts.virtualStoreDirMaxLength)),
        pkgMeta,
      }
      continue
    }
    graphNodeHashOpts ??= {
      cache: {},
      graph: lockfileToDepGraph(lockfile, opts.supportedArchitectures, opts.lockfileDir),
      supportedArchitectures: opts.supportedArchitectures,
      nodeVersion,
      lockfileDir: opts.lockfileDir,
    }
    const hash = calcGraphNodeHash(graphNodeHashOpts, pkgMeta)
    yield {
      dirInVirtualStore: path.join(opts.globalVirtualStoreDir, hash),
      pkgMeta,
    }
  }
}

function hashDependencyPaths (
  lockfile: LockfileObject,
  opts: GraphNodeHashOptions
): IterableIterator<HashedDepPath<PkgMetaAndSnapshot>> {
  const graph = lockfileToDepGraph(lockfile, opts.supportedArchitectures, opts.lockfileDir)
  return iterateHashedGraphNodes(graph, iteratePkgMeta(lockfile, graph), opts)
}
