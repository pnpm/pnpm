import { Lockfile, PackageSnapshot } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')

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
  const walked = new Set<string>(opts?.skipped ? Array.from(opts?.skipped) : [])

  return importerIds.map((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    const entryNodes = R.toPairs({
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

export default function lockfileWalker (
  lockfile: Lockfile,
  importerIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    skipped?: Set<string>
  }
) {
  const walked = new Set<string>(opts?.skipped ? Array.from(opts?.skipped) : [])
  const entryNodes = [] as string[]
  const directDeps = [] as Array<{ alias: string, depPath: string }>

  importerIds.forEach((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    R.toPairs({
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
    if (!pkgSnapshot) {
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
  return R.toPairs({
    ...nextPkg.dependencies,
    ...(opts.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
    .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
}
