import { LockfileMissingDependencyError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import type { DependenciesField, ProjectId } from '@pnpm/types'

export function lockfileToPackages (
  lockfile: LockfileObject,
  opts: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    resolvePeersFromWorkspaceRoot?: boolean
  }
): Map<string, Set<string>> {
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, Object.keys(lockfile.importers) as ProjectId[], {
    include: opts.include,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
  })
  const packages = new Map<string, Set<string>>()
  for (const importerWalker of importerWalkers) {
    if (importerWalker.step.missing.length > 0) {
      throw new LockfileMissingDependencyError(importerWalker.step.missing[0])
    }
    addPackages(packages, importerWalker.step)
  }
  return packages
}

function addPackages (packages: Map<string, Set<string>>, step: LockfileWalkerStep) {
  for (const { depPath, pkgSnapshot, next } of step.dependencies) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    if (version != null) {
      if (!packages.has(name)) {
        packages.set(name, new Set())
      }
      packages.get(name)!.add(version)
    }
    const nextStep = next()
    if (nextStep.missing.length > 0) {
      throw new LockfileMissingDependencyError(nextStep.missing[0])
    }
    addPackages(packages, nextStep)
  }
}
