import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import {
  Lockfile,
  PackageSnapshots,
} from '@pnpm/lockfile-types'
import lockfileWalker, { LockfileWalkerStep } from '@pnpm/lockfile-walker'
import pnpmLogger from '@pnpm/logger'
import { DependenciesField, Registries } from '@pnpm/types'
import R = require('ramda')
import filterImporter from './filterImporter'

const logger = pnpmLogger('lockfile')

export default function filterByImporters (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean },
    registries: Registries,
    skipped: Set<string>,
    failOnMissingDependencies: boolean,
  },
): Lockfile {
  const packages = {} as PackageSnapshots
  if (lockfile.packages) {
    pkgAllDeps(
      lockfileWalker(
        lockfile,
        importerIds,
        { include: opts.include, skipped: opts.skipped },
      ).step,
      packages,
      {
        failOnMissingDependencies: opts.failOnMissingDependencies,
      },
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
    failOnMissingDependencies: boolean,
  },
) {
  for (const { pkgSnapshot, relDepPath, next } of step.dependencies) {
    pickedPackages[relDepPath] = pkgSnapshot
    pkgAllDeps(next(), pickedPackages, opts)
  }
  for (const relDepPath of step.missing) {
    const message = `No entry for "${relDepPath}" in ${WANTED_LOCKFILE}`
    if (opts.failOnMissingDependencies) {
      throw new PnpmError('LOCKFILE_MISSING_DEPENDENCY', message)
    }
    logger.debug(message)
  }
}
