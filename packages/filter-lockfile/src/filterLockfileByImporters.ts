import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  Lockfile,
  PackageSnapshots,
} from '@pnpm/lockfile-types'
import lockfileWalker, { LockfileWalkerStep } from '@pnpm/lockfile-walker'
import pnpmLogger from '@pnpm/logger'
import { DependenciesField } from '@pnpm/types'
import filterImporter from './filterImporter'
import LockfileMissingDependencyError from './LockfileMissingDependencyError'

const logger = pnpmLogger('lockfile')

export default function filterByImporters (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    skipped: Set<string>
    failOnMissingDependencies: boolean
  }
): Lockfile {
  const packages = {} as PackageSnapshots
  if (lockfile.packages) {
    pkgAllDeps(
      lockfileWalker(
        lockfile,
        importerIds,
        { include: opts.include, skipped: opts.skipped }
      ).step,
      packages,
      {
        failOnMissingDependencies: opts.failOnMissingDependencies,
      }
    )
  }

  const importers = importerIds.reduce((acc, importerId) => {
    acc[importerId] = filterImporter(lockfile.importers[importerId], opts.include)
    return acc
  }, { ...lockfile.importers })

  return {
    importers,
    lockfileVersion: lockfile.lockfileVersion,
    packages,
  }
}

function pkgAllDeps (
  step: LockfileWalkerStep,
  pickedPackages: PackageSnapshots,
  opts: {
    failOnMissingDependencies: boolean
  }
) {
  for (const { pkgSnapshot, depPath, next } of step.dependencies) {
    pickedPackages[depPath] = pkgSnapshot
    pkgAllDeps(next(), pickedPackages, opts)
  }
  for (const depPath of step.missing) {
    if (opts.failOnMissingDependencies) {
      throw new LockfileMissingDependencyError(depPath)
    }
    logger.debug(`No entry for "${depPath}" in ${WANTED_LOCKFILE}`)
  }
}
