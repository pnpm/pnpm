import { type LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { type DependenciesField, type ProjectId } from '@pnpm/types'

export function lockfileToPackages (
  lockfile: LockfileObject,
  opts: {
    include?: { [dependenciesField in DependenciesField]: boolean }
  }
): Map<string, Set<string>> {
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, Object.keys(lockfile.importers) as ProjectId[], { include: opts?.include })
  const packages = new Map<string, Set<string>>()
  for (const importerWalker of importerWalkers) {
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
    addPackages(packages, next())
  }
}
