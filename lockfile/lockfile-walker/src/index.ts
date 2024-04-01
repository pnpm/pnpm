import { type Lockfile, type PackageSnapshot } from '@pnpm/lockfile-types'
import { type DependenciesField } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import path from 'node:path'
import normalizePath from 'normalize-path'

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
    recursive?: boolean
  }
): Array<{ importerId: string, step: LockfileWalkerStep }> {
  const walked = new Set<string>(((opts?.skipped) != null) ? Array.from(opts?.skipped) : [])

  return importerIds.map((importerId) => {
    const entryNodes = lockfileDeps(lockfile, [importerId], opts).map(({ depPath }) => depPath)
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
    const deps = lockfileDeps(lockfile, [importerId], {
      ...opts,
      recursive: false,
    })
    const nodes = deps.map(({ depPath }) => depPath)
    directDeps.push(...deps)
    entryNodes.push(...nodes)
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

// may return duplicate dependencies if recursive == true
function lockfileDeps (
  lockfile: Lockfile,
  visitedImporterIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<string>
    recursive?: boolean
  }
): Array<{ alias: string, depPath: string }> {
  const importerId = visitedImporterIds[visitedImporterIds.length - 1]
  const projectSnapshot = lockfile.importers[importerId]
  const deps = [] as Array<{ alias: string, depPath: string }>
  const isBaseCall = visitedImporterIds.length === 1

  Object.entries({
    ...(!isBaseCall || opts?.include?.devDependencies === false ? {} : projectSnapshot.devDependencies),
    ...(opts?.include?.dependencies === false ? {} : projectSnapshot.dependencies),
    ...(opts?.include?.optionalDependencies === false ? {} : projectSnapshot.optionalDependencies),
  })
    .forEach(([pkgName, reference]) => {
      const depPath = dp.refToRelative(reference, pkgName)
      if (depPath !== null) {
        deps.push({ alias: pkgName, depPath })
      } else if (opts?.recursive) {
        const relativePath = reference.slice('link:'.length)
        const childImporterId = normalizePath(path.normalize(path.join(importerId, relativePath)))
        if (visitedImporterIds.includes(childImporterId)) {
          return
        }
        const visitedIds = [...visitedImporterIds, childImporterId]
        const childDeps = lockfileDeps(lockfile, visitedIds, opts)
        deps.push(...childDeps)
      }
    })
  return deps
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

function next (opts: { includeOptionalDependencies: boolean }, nextPkg: PackageSnapshot): string[] {
  return Object.entries({
    ...nextPkg.dependencies,
    ...(opts.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
    .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
}
