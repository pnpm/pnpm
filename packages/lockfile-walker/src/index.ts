import { Lockfile, PackageSnapshot } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')

export type LockfileDependency = {
  relDepPath: string,
  pkgSnapshot: PackageSnapshot,
  next: () => LockfileWalkStep,
}

export type LockfileWalkStep = {
  dependencies: LockfileDependency[],
  links: string[],
  missing: string[],
}

export default function walker (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean },
    skipped?: Set<string>,
  },
) {
  const walked = new Set<string>(opts.skipped ? Array.from(opts.skipped) : [])
  const entryNodes = [] as string[]

  importerIds.forEach((importerId) => {
    const lockfileImporter = lockfile.importers[importerId]
    R.toPairs({
      ...(opts.include.devDependencies && lockfileImporter.devDependencies || {}),
      ...(opts.include.dependencies && lockfileImporter.dependencies || {}),
      ...(opts.include.optionalDependencies && lockfileImporter.optionalDependencies || {}),
    })
    .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null)
    .forEach((relDepPath) => {
      entryNodes.push(relDepPath as string)
    })
  })

  return step(entryNodes)

  function step (nextRelDepPaths: string[]) {
    const result: LockfileWalkStep = {
      dependencies: [],
      links: [],
      missing: [],
    }
    for (let relDepPath of nextRelDepPaths) {
      if (walked.has(relDepPath)) continue
      walked.add(relDepPath)
      const pkgSnapshot = lockfile.packages?.[relDepPath]
      if (!pkgSnapshot) {
        if (relDepPath.startsWith('link:')) {
          result.links.push(relDepPath)
          continue
        }
        result.missing.push(relDepPath)
        continue
      }
      result.dependencies.push({
        next: () => step(next(pkgSnapshot)),
        pkgSnapshot,
        relDepPath,
      })
    }
    return result
  }

  function next (nextPkg: PackageSnapshot) {
    return R.toPairs({
      ...nextPkg.dependencies,
      ...(opts.include.optionalDependencies && nextPkg.optionalDependencies || {}),
    })
    .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
  }
}
