import { type Lockfile, type PackageSnapshot } from '@pnpm/lockfile-types'
import { type DependenciesField, type DepPath } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'

export interface LockedDependency {
  depPath: DepPath
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
): Array<{ importerId: string, step: LockfileWalkerStep }> {
  const walked = new Set<string>(((opts?.skipped) != null) ? Array.from(opts?.skipped) : [])

  return importerIds.map((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    const entryNodes = Object.entries({
      ...(opts?.include?.devDependencies === false ? {} : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false ? {} : projectSnapshot.dependencies),
      ...(opts?.include?.optionalDependencies === false ? {} : projectSnapshot.optionalDependencies),
    })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((nodeId) => nodeId !== null) as DepPath[]
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

export interface LockfileWalker {
  directDeps: Array<{
    alias: string
    depPath: DepPath
  }>
  step: LockfileWalkerStep
}

export function lockfileWalker (
  lockfile: Lockfile,
  importerIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<string>
  }
): LockfileWalker {
  const walked = new Set<string>(((opts?.skipped) != null) ? Array.from(opts?.skipped) : [])
  const entryNodes = [] as DepPath[]
  const directDeps = [] as Array<{ alias: string, depPath: DepPath }>

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
  nextDepPaths: DepPath[]
): LockfileWalkerStep {
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

function next (opts: { includeOptionalDependencies: boolean }, nextPkg: PackageSnapshot): DepPath[] {
  return Object.entries({
    ...nextPkg.dependencies,
    ...(opts.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
    .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as DepPath[]
}
