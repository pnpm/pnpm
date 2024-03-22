import { type Lockfile, type PackageSnapshots, type ResolvedDependencies } from '@pnpm/lockfile-types'
import * as dp from '@pnpm/dependency-path'
import unnest from 'ramda/src/unnest'

export enum DepType {
  DevOnly,
  DevAndProd,
  ProdOnly
}

export type DepTypes = Record<string, DepType>

export function detectDepTypes (lockfile: Lockfile): DepTypes {
  const dev: DepTypes = {}
  const devDepPaths = unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.devDependencies ?? {})))
  const optionalDepPaths = unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.optionalDependencies ?? {})))
  const prodDepPaths = unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.dependencies ?? {})))
  const ctx = {
    packages: lockfile.packages ?? {},
    walked: new Set<string>(),
    notProdOnly: new Set<string>(),
    dev,
  }
  detectDepTypesInSubGraph(ctx, devDepPaths, {
    dev: true,
    optional: false,
  })
  detectDepTypesInSubGraph(ctx, optionalDepPaths, {
    dev: false,
    optional: true,
  })
  detectDepTypesInSubGraph(ctx, prodDepPaths, {
    dev: false,
    optional: false,
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
  depPaths: string[],
  opts: {
    dev: boolean
    optional: boolean
  }
) {
  for (const depPath of depPaths) {
    const key = `${depPath}:${opts.optional.toString()}:${opts.dev.toString()}`
    if (ctx.walked.has(key)) continue
    ctx.walked.add(key)
    if (!ctx.packages[depPath]) {
      continue
    }
    const depLockfile = ctx.packages[depPath]
    if (opts.dev) {
      ctx.notProdOnly.add(depPath)
      ctx.dev[depPath] = DepType.DevOnly
    } else if (ctx.dev[depPath] === DepType.DevOnly) { // keeping if dev is explicitly false
      ctx.dev[depPath] = DepType.DevAndProd
    } else if (ctx.dev[depPath] === undefined && !ctx.notProdOnly.has(depPath)) {
      ctx.dev[depPath] = DepType.ProdOnly
    }
    const newDependencies = resolvedDepsToDepPaths(depLockfile.dependencies ?? {})
    detectDepTypesInSubGraph(ctx, newDependencies, opts)
    const newOptionalDependencies = resolvedDepsToDepPaths(depLockfile.optionalDependencies ?? {})
    detectDepTypesInSubGraph(ctx, newOptionalDependencies, { dev: opts.dev, optional: true })
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies) {
  return Object.entries(deps)
    .map(([alias, ref]) => dp.refToRelative(ref, alias))
    .filter((depPath) => depPath !== null) as string[]
}
