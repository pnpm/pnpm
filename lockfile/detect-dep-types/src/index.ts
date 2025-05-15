import { type LockfileObject, type PackageSnapshots, type ResolvedDependencies } from '@pnpm/lockfile.types'
import * as dp from '@pnpm/dependency-path'
import { type DepPath } from '@pnpm/types'

export enum DepType {
  DevOnly,
  DevAndProd,
  ProdOnly
}

export type DepTypes = Record<string, DepType>

export function detectDepTypes (lockfile: LockfileObject): DepTypes {
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
    const newDependencies = resolvedDepsToDepPaths(depLockfile.dependencies ?? {})
    detectDepTypesInSubGraph(ctx, newDependencies, opts)
    const newOptionalDependencies = resolvedDepsToDepPaths(depLockfile.optionalDependencies ?? {})
    detectDepTypesInSubGraph(ctx, newOptionalDependencies, { dev: opts.dev })
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies): DepPath[] {
  return Object.entries(deps)
    .map(([alias, ref]) => dp.refToRelative(ref, alias))
    .filter((depPath) => depPath !== null) as DepPath[]
}
