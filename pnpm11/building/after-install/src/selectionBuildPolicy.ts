import { isPackagePlaced } from '@pnpm/building.during-install'
import { createAllowBuildFunction } from '@pnpm/building.policy'
import { type DepsGraph, lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

import type { StrictBuildOptions } from './extendBuildOptions.js'

/**
 * The `allowBuilds` policy for placing the installed graph when only some
 * packages are rebuilt. It allows the selected packages and the packages they
 * depend on, as the build policy does, and the packages already placed built.
 * Every other package stays unbuilt, so its build is left pending.
 */
export async function getSelectionBuildPolicy (
  lockfile: LockfileObject,
  opts: { allowBuilds?: StrictBuildOptions['allowBuilds'], dangerouslyAllowAllBuilds?: boolean, supportedArchitectures?: StrictBuildOptions['supportedArchitectures'] },
  { selected, builtSlotPackageDirs }: { selected: DepPath[], builtSlotPackageDirs: Map<DepPath, string> }
): Promise<Record<string, boolean>> {
  const allowBuild = createAllowBuildFunction(opts)
  const needed = collectDependencies(lockfileToDepGraph(lockfile, opts.supportedArchitectures), selected)
  const allowed = await Promise.all(Object.keys(lockfile.packages ?? {}).map(async (depPath) => {
    if (allowBuild?.(depPath as DepPath) !== true) return undefined
    if (needed.has(depPath as DepPath)) return depPath
    const builtDir = builtSlotPackageDirs.get(depPath as DepPath)
    return builtDir != null && await isPackagePlaced(builtDir) ? depPath : undefined
  }))
  return Object.fromEntries(allowed
    .filter((depPath): depPath is string => depPath != null)
    .map((depPath) => [dp.getPkgIdWithPatchHash(depPath as DepPath), true]))
}

function collectDependencies (graph: DepsGraph<DepPath>, roots: DepPath[]): Set<DepPath> {
  const collected = new Set<DepPath>()
  const pending = [...roots]
  for (let depPath = pending.pop(); depPath != null; depPath = pending.pop()) {
    if (collected.has(depPath)) continue
    collected.add(depPath)
    pending.push(...Object.values(graph[depPath]?.children ?? {}))
  }
  return collected
}
