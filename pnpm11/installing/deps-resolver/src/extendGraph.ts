import path from 'node:path'

import { iterateHashedGraphNodes } from '@pnpm/deps.graph-hasher'
import { isRuntimeDepPath } from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import type {
  AllowBuild,
  DepPath,
  PkgIdWithPatchHash,
  SupportedArchitectures,
} from '@pnpm/types'

import type { DependenciesGraph } from './index.js'

export function extendGraph (
  graph: DependenciesGraph,
  opts: {
    allowBuild?: AllowBuild
    globalVirtualStoreDir: string
    enableGlobalVirtualStore?: boolean
    lockfileDir: string
    supportedArchitectures?: SupportedArchitectures
    wantedLockfile: LockfileObject
  }
): DependenciesGraph {
  const pkgMetaIter = iterateGraphPkgMetaEntries(graph, !opts.enableGlobalVirtualStore)
  // Only use allowBuild for engine-agnostic hash optimization when GVS is on
  const allowBuild = opts.enableGlobalVirtualStore ? opts.allowBuild : undefined
  // Anchor every snapshot's engine hash to the root project's pinned
  // Node version (from `engines.runtime` / `devEngines.runtime`).
  // Without this, GVS slots for approved-build packages would hash
  // under the runner's `process.version` instead of the script-runner
  // Node, splitting the cache between pinned and non-pinned installs
  // on the same host.
  const nodeVersion = findLockedRootNodeRuntime(opts.wantedLockfile)?.version
  for (const { pkgMeta: { depPath }, hash } of iterateHashedGraphNodes(graph, pkgMetaIter, {
    allowBuild,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion,
    lockfileDir: opts.lockfileDir,
  })) {
    const modules = path.join(opts.globalVirtualStoreDir, hash, 'node_modules')
    const node = graph[depPath]
    Object.assign(node, {
      modules,
      dir: safeJoinModulesDir(modules, node.name),
    })
  }
  return graph
}

function * iterateGraphPkgMetaEntries (graph: DependenciesGraph, runtimeOnly?: boolean): IterableIterator<{ depPath: DepPath; name: string; version: string; pkgIdWithPatchHash: PkgIdWithPatchHash }> {
  for (const depPath in graph) {
    if (Object.hasOwn(graph, depPath)) {
      if (runtimeOnly && !isRuntimeDepPath(depPath as DepPath)) continue
      const { name, version, pkgIdWithPatchHash } = graph[depPath as DepPath]
      yield { depPath: depPath as DepPath, name, version, pkgIdWithPatchHash }
    }
  }
}
