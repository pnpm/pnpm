import { Lockfile, PackageSnapshot } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')

export type LockedDependency = {
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
  next: () => LockfileWalkerStep,
}

export type LockfileWalkerStep = {
  dependencies: LockedDependency[],
  links: string[],
  missing: string[],
}

export function lockfileWalkerGroupImporterSteps (
  lockfile: Lockfile,
  importerIds: string[],
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean },
    skipped?: Set<string>,
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
    .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
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
    include?: { [dependenciesField in DependenciesField]: boolean },
    skipped?: Set<string>,
  }
) {
  const walked = new Set<string>(opts?.skipped ? Array.from(opts?.skipped) : [])
  const entryNodes = [] as string[]
  const directDeps = [] as Array<{ alias: string, relDepPath: string }>

  importerIds.forEach((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    R.toPairs({
      ...(opts?.include?.devDependencies === false ? {} : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false ? {} : projectSnapshot.dependencies),
      ...(opts?.include?.optionalDependencies === false ? {} : projectSnapshot.optionalDependencies),
    })
    .forEach(([ pkgName, reference ]) => {
      const relDepPath = dp.refToRelative(reference, pkgName)
      if (relDepPath === null) return
      entryNodes.push(relDepPath as string)
      directDeps.push({ alias: pkgName, relDepPath })
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
    includeOptionalDependencies: boolean,
    lockfile: Lockfile,
    walked: Set<string>,
  },
  nextRelDepPaths: string[]
) {
  const result: LockfileWalkerStep = {
    dependencies: [],
    links: [],
    missing: [],
  }
  for (let relDepPath of nextRelDepPaths) {
    if (ctx.walked.has(relDepPath)) continue
    ctx.walked.add(relDepPath)
    const pkgSnapshot = ctx.lockfile.packages?.[relDepPath]
    if (!pkgSnapshot) {
      if (relDepPath.startsWith('link:')) {
        result.links.push(relDepPath)
        continue
      }
      result.missing.push(relDepPath)
      continue
    }
    result.dependencies.push({
      next: () => step(ctx, next({ includeOptionalDependencies: ctx.includeOptionalDependencies }, pkgSnapshot)),
      pkgSnapshot,
      relDepPath,
    })
  }
  return result
}

function next (opts: { includeOptionalDependencies: boolean }, nextPkg: PackageSnapshot) {
  return R.toPairs({
    ...nextPkg.dependencies,
    ...(opts.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
  .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
  .filter((nodeId) => nodeId !== null) as string[]
}
