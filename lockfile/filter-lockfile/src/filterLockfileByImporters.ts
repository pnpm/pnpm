import { WANTED_LOCKFILE } from '@pnpm/constants'
import { LockfileMissingDependencyError } from '@pnpm/error'
import {
  Lockfile,
  PackageSnapshots,
} from '@pnpm/lockfile-types'
import { lockfileWalker, LockfileWalkerStep } from '@pnpm/lockfile-walker'
import { logger } from '@pnpm/logger'
import { DependenciesField } from '@pnpm/types'
import { filterImporter } from './filterImporter'

const lockfileLogger = logger('lockfile')

export function filterLockfileByImporters (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    skipped: Set<string>
    failOnMissingDependencies: boolean
  }
): Lockfile {
  const packages = {} as PackageSnapshots
  if (lockfile.packages != null) {
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
    ...lockfile,
    importers,
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
    lockfileLogger.debug(`No entry for "${depPath}" in ${WANTED_LOCKFILE}`)
  }
}
