import { type Lockfile, type PackageSnapshot, type PackageSnapshots, type ResolvedDependencies } from '@pnpm/lockfile-types'
import { type DependenciesField } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import unnest from 'ramda/src/unnest'

export interface LockedDependency {
  depPath: string
  pkgSnapshot: PackageSnapshot
  next: () => LockfileWalkerStep
}

export interface LockfileWalkerStep {
  dependencies: LockedDependency[]
  links: string[]
  missing: string[]
}

export function lockfileWalkerGroupImporterSteps (
  lockfile: Lockfile,
  importerIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<string>
  }
) {
  const walked = new Set<string>(((opts?.skipped) != null) ? Array.from(opts?.skipped) : [])

  return importerIds.map((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    const entryNodes = Object.entries({
      ...(opts?.include?.devDependencies === false ? {} : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false ? {} : projectSnapshot.dependencies),
      ...(opts?.include?.optionalDependencies === false ? {} : projectSnapshot.optionalDependencies),
    })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((nodeId) => nodeId !== null) as string[]
    return {
      importerId,
      step: step({
        includeOptionalDependencies: opts?.include?.optionalDependencies !== false,
        lockfile,
        walked,
      }, entryNodes),
    }
  })
}

export function lockfileWalker (
  lockfile: Lockfile,
  importerIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<string>
  }
) {
  const walked = new Set<string>(((opts?.skipped) != null) ? Array.from(opts?.skipped) : [])
  const entryNodes = [] as string[]
  const directDeps = [] as Array<{ alias: string, depPath: string }>

  importerIds.forEach((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    Object.entries({
      ...(opts?.include?.devDependencies === false ? {} : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false ? {} : projectSnapshot.dependencies),
      ...(opts?.include?.optionalDependencies === false ? {} : projectSnapshot.optionalDependencies),
    })
      .forEach(([pkgName, reference]) => {
        const depPath = dp.refToRelative(reference, pkgName)
        if (depPath === null) return
        entryNodes.push(depPath)
        directDeps.push({ alias: pkgName, depPath })
      })
  })
  return {
    directDeps,
    step: step({
      includeOptionalDependencies: opts?.include?.optionalDependencies !== false,
      lockfile,
      walked,
    }, entryNodes),
  }
}

function step (
  ctx: {
    includeOptionalDependencies: boolean
    lockfile: Lockfile
    walked: Set<string>
  },
  nextDepPaths: string[]
) {
  const result: LockfileWalkerStep = {
    dependencies: [],
    links: [],
    missing: [],
  }
  for (const depPath of nextDepPaths) {
    if (ctx.walked.has(depPath)) continue
    ctx.walked.add(depPath)
    const pkgSnapshot = ctx.lockfile.packages?.[depPath]
    if (pkgSnapshot == null) {
      if (depPath.startsWith('link:')) {
        result.links.push(depPath)
        continue
      }
      result.missing.push(depPath)
      continue
    }
    result.dependencies.push({
      depPath,
      next: () => step(ctx, next({ includeOptionalDependencies: ctx.includeOptionalDependencies }, pkgSnapshot)),
      pkgSnapshot,
    })
  }
  return result
}

function next (opts: { includeOptionalDependencies: boolean }, nextPkg: PackageSnapshot) {
  return Object.entries({
    ...nextPkg.dependencies,
    ...(opts.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
    .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
}

export function getDevOnlyDepPaths (lockfile: Lockfile) {
  const dev: Record<string, boolean | undefined> = {}
  const devDepPaths = unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.devDependencies ?? {})))
  const optionalDepPaths = unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.optionalDependencies ?? {})))
  const prodDepPaths = unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.dependencies ?? {})))
  const ctx = {
    packages: lockfile.packages ?? {},
    walked: new Set<string>(),
    notProdOnly: new Set<string>(),
    dev,
  }
  copyDependencySubGraph(ctx, devDepPaths, {
    dev: true,
    optional: false,
  })
  copyDependencySubGraph(ctx, optionalDepPaths, {
    dev: false,
    optional: true,
  })
  copyDependencySubGraph(ctx, prodDepPaths, {
    dev: false,
    optional: false,
  })
  return dev
}

function copyDependencySubGraph (
  ctx: {
    notProdOnly: Set<string>
    packages: PackageSnapshots
    walked: Set<string>
    dev: Record<string, boolean | undefined>
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
      ctx.dev[depPath] = true
    } else if (ctx.dev[depPath] === true) { // keeping if dev is explicitly false
      ctx.dev[depPath] = undefined
    } else if (ctx.dev[depPath] === undefined && !ctx.notProdOnly.has(depPath)) {
      ctx.dev[depPath] = false
    }
    const newDependencies = resolvedDepsToDepPaths(depLockfile.dependencies ?? {})
    copyDependencySubGraph(ctx, newDependencies, opts)
    const newOptionalDependencies = resolvedDepsToDepPaths(depLockfile.optionalDependencies ?? {})
    copyDependencySubGraph(ctx, newOptionalDependencies, { dev: opts.dev, optional: true })
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies) {
  return Object.entries(deps)
    .map(([alias, ref]) => dp.refToRelative(ref, alias))
    .filter((depPath) => depPath !== null) as string[]
}
