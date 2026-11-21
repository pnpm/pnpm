import { getLockfileImporterId, type LockfileObject } from '@pnpm/lockfile.fs'
import { getPeerSatisfactionEdgesToSkip } from '@pnpm/lockfile.peer-edges'
import type { DependenciesField } from '@pnpm/types'

import { buildDependencyGraph, type DependencyGraph } from './buildDependencyGraph.js'
import type { TreeNodeId } from './TreeNodeId.js'

export function buildSelectedProjectsGraph (
  projectPaths: string[],
  opts: {
    lockfileDir: string
    lockfile: LockfileObject
    include?: { [field in DependenciesField]?: boolean }
    resolvePeersFromWorkspaceRoot?: boolean
    excludePeerDependencies?: boolean
  }
): DependencyGraph {
  const include = opts.include ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }

  const allRootIds: TreeNodeId[] = []
  for (const projectPath of projectPaths) {
    const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
    if (opts.lockfile.importers[importerId]) {
      allRootIds.push({ type: 'importer', importerId })
    }
  }

  return buildDependencyGraph(allRootIds, {
    currentPackages: opts.lockfile.packages ?? {},
    importers: opts.lockfile.importers,
    include,
    lockfileDir: opts.lockfileDir,
    skipPeerEdges: opts.excludePeerDependencies,
    peerSatisfactionEdges: getPeerSatisfactionEdgesToSkip(opts.lockfile, {
      include,
      resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    }),
  })
}
