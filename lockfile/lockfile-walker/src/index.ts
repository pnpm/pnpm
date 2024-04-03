import { type ProjectSnapshot, type Lockfile, type PackageSnapshot } from '@pnpm/lockfile-types'
import { type DependenciesField } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import path from 'node:path'
import normalizePath from 'normalize-path'

export interface LockedLink {
  importerId: string
  projectSnapshot: ProjectSnapshot
  next: () => LockfileWalkerStep
}

export interface LockedDependency {
  depPath: string
  pkgSnapshot: PackageSnapshot
  next: () => LockfileWalkerStep
}

export interface LockfileWalkerStep {
  dependencies: LockedDependency[]
  links: LockedLink[]
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
      .map(([pkgName, reference]) => getDepPath(pkgName, reference, importerId))
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
    depPath: string
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
        const depPath = getDepPath(pkgName, reference, importerId)
        if (!depPath.startsWith('link:')) {
          directDeps.push({ alias: pkgName, depPath })
        }
        entryNodes.push(depPath)
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
        const importerId = depPath.slice('link:'.length)
        const projectSnapshot = ctx.lockfile.importers[importerId]
        result.links.push({
          importerId,
          next: () => step(ctx, next({ includeOptionalDependencies: ctx.includeOptionalDependencies, importerId }, projectSnapshot)),
          projectSnapshot,
        })
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

function next (opts: { includeOptionalDependencies: boolean, importerId?: string }, nextPkg: PackageSnapshot | ProjectSnapshot): string[] {
  return Object.entries({
    ...nextPkg.dependencies,
    ...(opts.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
    .map(([pkgName, reference]) => getDepPath(pkgName, reference, opts.importerId))
    .filter((depPath): depPath is string => depPath !== null)
}

function getDepPath (pkgName: string, reference: string, importerId: string): string
// eslint-disable-next-line
function getDepPath (pkgName: string, reference: string, importerId?: string): string | null
// eslint-disable-next-line
function getDepPath (pkgName: string, reference: string, importerId?: string): string | null {
  const depPath = dp.refToRelative(reference, pkgName)
  if (depPath !== null) {
    return depPath
  } else if (importerId !== undefined) {
    const relativePath = reference.slice('link:'.length)
    const childImporterId = normalizePath(path.normalize(path.join(importerId, relativePath)))
    return `link:${childImporterId}`
  } else {
    return null
  }
}
