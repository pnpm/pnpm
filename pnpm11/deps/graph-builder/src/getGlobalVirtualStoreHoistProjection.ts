import { lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import * as dp from '@pnpm/deps.path'
import type { DependenciesGraph as HoistGraph } from '@pnpm/installing.linking.hoist'
import { getGlobalVirtualStoreHoistProjection as getProjection } from '@pnpm/installing.linking.hoist'
import type { IncludedDependencies } from '@pnpm/installing.modules-yaml'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import type { DepPath, ProjectId } from '@pnpm/types'

export function getGlobalVirtualStoreHoistProjection (
  lockfile: LockfileObject,
  opts: {
    hoistPattern?: string[]
    importerIds: ProjectId[]
    include: IncludedDependencies
    publicHoistPattern?: string[]
    skipped: Set<DepPath>
  }
): Record<string, DepPath> {
  const dependencyGraph = lockfileToDepGraph(lockfile)
  const graph: HoistGraph<DepPath> = {}
  for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    if (opts.skipped.has(depPath)) continue
    if ('directory' in pkgSnapshot.resolution) continue
    const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const children = { ...dependencyGraph[depPath]?.children }
    if (!opts.include.optionalDependencies) {
      for (const alias of Object.keys(pkgSnapshot.optionalDependencies ?? {})) {
        delete children[alias]
      }
    }
    graph[depPath] = {
      children,
      depPath,
      dir: '',
      hasBin: pkgSnapshot.hasBin === true,
      name,
      optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
    }
  }

  const directDepsByImporterId: Record<string, Map<string, DepPath>> = {}
  for (const importerId of opts.importerIds) {
    const importer = lockfile.importers[importerId]
    const dependencies = {
      ...(opts.include.devDependencies ? importer.devDependencies : {}),
      ...(opts.include.dependencies ? importer.dependencies : {}),
      ...(opts.include.optionalDependencies ? importer.optionalDependencies : {}),
    }
    directDepsByImporterId[importerId] = new Map(
      Object.entries(dependencies)
        .map(([alias, reference]) => [alias, dp.refToRelative(reference, alias)] as const)
        .filter((entry): entry is [string, DepPath] => entry[1] != null && graph[entry[1]] != null)
    )
  }

  return getProjection({
    directDepsByImporterId,
    graph,
    privateHoistedModulesDir: '',
    privateHoistPattern: opts.hoistPattern ?? [],
    publicHoistedModulesDir: '',
    publicHoistPattern: opts.publicHoistPattern ?? [],
    skipped: opts.skipped,
  })
}
