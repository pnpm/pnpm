import * as dp from '@pnpm/deps.path'
import {
  getPeerSatisfactionEdges,
  isPeerSatisfactionEdge,
  type PeerSatisfactionEdges,
  type PeerSatisfactionEdgesOptions,
} from '@pnpm/lockfile.peer-edges'
import type { LockfileObject, PackageSnapshots, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

export const DepType = {
  DevOnly: 0,
  DevAndProd: 1,
  ProdOnly: 2,
} as const

export type DepType = (typeof DepType)[keyof typeof DepType]

export type DepTypes = Record<string, DepType>

/**
 * Classifies every package reachable from the importers. Peer-satisfaction
 * edges (see `@pnpm/lockfile.peer-edges`) are not followed: a devDependency
 * that only satisfies an optional peer of a production package is dev-only.
 */
export function detectDepTypes (lockfile: LockfileObject, opts?: PeerSatisfactionEdgesOptions): DepTypes {
  const dev: DepTypes = {}
  const devDepPaths = Object.values(lockfile.importers)
    .map((deps) => resolvedDepsToDepPaths(deps.devDependencies ?? {})).flat()
  const optionalDepPaths = Object.values(lockfile.importers)
    .map((deps) => resolvedDepsToDepPaths(deps.optionalDependencies ?? {})).flat()
  const prodDepPaths = Object.values(lockfile.importers)
    .map((deps) => resolvedDepsToDepPaths(deps.dependencies ?? {})).flat()
  const ctx = {
    packages: lockfile.packages ?? {},
    walked: new Set<string>(),
    notProdOnly: new Set<string>(),
    dev,
    peerSatisfactionEdges: getPeerSatisfactionEdges(lockfile, opts),
  }
  detectDepTypesInSubGraph(ctx, devDepPaths, {
    dev: true,
  })
  detectDepTypesInSubGraph(ctx, optionalDepPaths, {
    dev: false,
  })
  detectDepTypesInSubGraph(ctx, prodDepPaths, {
    dev: false,
  })
  return dev
}

function detectDepTypesInSubGraph (
  ctx: {
    notProdOnly: Set<string>
    packages: PackageSnapshots
    walked: Set<string>
    dev: Record<string, DepType>
    peerSatisfactionEdges: PeerSatisfactionEdges
  },
  depPaths: DepPath[],
  opts: {
    dev: boolean
  }
): void {
  for (const depPath of depPaths) {
    const key = `${depPath}:${opts.dev.toString()}`
    if (ctx.walked.has(key)) continue
    ctx.walked.add(key)
    if (!ctx.packages[depPath]) {
      continue
    }
    if (opts.dev) {
      ctx.notProdOnly.add(depPath)
      ctx.dev[depPath] = DepType.DevOnly
    } else if (ctx.dev[depPath] === DepType.DevOnly) { // keeping if dev is explicitly false
      ctx.dev[depPath] = DepType.DevAndProd
    } else if (ctx.dev[depPath] === undefined && !ctx.notProdOnly.has(depPath)) {
      ctx.dev[depPath] = DepType.ProdOnly
    }
    const depLockfile = ctx.packages[depPath]
    const newDependencies = resolvedDepsToDepPaths(depLockfile.dependencies ?? {}, ctx.peerSatisfactionEdges, depPath)
    detectDepTypesInSubGraph(ctx, newDependencies, opts)
    const newOptionalDependencies = resolvedDepsToDepPaths(depLockfile.optionalDependencies ?? {}, ctx.peerSatisfactionEdges, depPath)
    detectDepTypesInSubGraph(ctx, newOptionalDependencies, { dev: opts.dev })
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies, peerSatisfactionEdges?: PeerSatisfactionEdges, parent?: DepPath): DepPath[] {
  return Object.entries(deps)
    .filter(([alias]) => parent == null || !isPeerSatisfactionEdge(peerSatisfactionEdges, parent, alias))
    .map(([alias, ref]) => dp.refToRelative(ref, alias))
    .filter((depPath) => depPath !== null) as DepPath[]
}
