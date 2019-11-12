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
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean },
    skipped?: Set<string>,
  },
) {
  const walked = new Set<string>(opts?.skipped ? Array.from(opts?.skipped) : [])

  return importerIds.map((importerId) => {
    const lockfileImporter = lockfile.importers[importerId]
    const entryNodes = R.toPairs({
      ...(opts?.include?.devDependencies === false ? {} : lockfileImporter.devDependencies),
      ...(opts?.include?.dependencies === false ? {} : lockfileImporter.dependencies),
      ...(opts?.include?.optionalDependencies === false ? {} : lockfileImporter.optionalDependencies),
    })
    .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
    return {
      importerId,
      step: step(entryNodes),
    }
  })

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
      ...(opts?.include?.optionalDependencies === false ? {} : nextPkg.optionalDependencies),
    })
    .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
  }
}
