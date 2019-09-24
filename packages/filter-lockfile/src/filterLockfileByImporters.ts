import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import {
  Lockfile,
  PackageSnapshots,
} from '@pnpm/lockfile-types'
import pnpmLogger from '@pnpm/logger'
import { DependenciesField, Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')
import filterImporter from './filterImporter'
import filterLockfile from './filterLockfile'

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
  if (R.equals(importerIds.sort(), R.keys(lockfile.importers).sort())) {
    return filterLockfile(lockfile, opts)
  }
  const importerDeps = importerIds
    .map((importerId) => lockfile.importers[importerId])
    .map((importer) => ({
      ...(opts.include.dependencies && importer.dependencies || {}),
      ...(opts.include.devDependencies && importer.devDependencies || {}),
      ...(opts.include.optionalDependencies && importer.optionalDependencies || {}),
    }))
    .map(R.toPairs)
  const directDepPaths = R.unnest(importerDeps)
    .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
  const packages = lockfile.packages &&
    pickPkgsWithAllDeps(lockfile.packages, directDepPaths, {
      failOnMissingDependencies: opts.failOnMissingDependencies,
      include: opts.include,
      skipped: opts.skipped,
    }) || {}

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

function pickPkgsWithAllDeps (
  pkgSnapshots: PackageSnapshots,
  relDepPaths: string[],
  opts: {
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    skipped: Set<string>,
  },
) {
  const pickedPackages = {} as PackageSnapshots
  pkgAllDeps(pkgSnapshots, pickedPackages, relDepPaths, opts)
  return pickedPackages
}

function pkgAllDeps (
  pkgSnapshots: PackageSnapshots,
  pickedPackages: PackageSnapshots,
  relDepPaths: string[],
  opts: {
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    skipped: Set<string>,
  },
) {
  for (const relDepPath of relDepPaths) {
    if (pickedPackages[relDepPath] || opts.skipped.has(relDepPath)) continue
    const pkgSnapshot = pkgSnapshots[relDepPath]
    if (!pkgSnapshot && !relDepPath.startsWith('link:')) {
      const message = `No entry for "${relDepPath}" in ${WANTED_LOCKFILE}`
      if (opts.failOnMissingDependencies) {
        throw new PnpmError('LOCKFILE_MISSING_DEPENDENCY', message)
      }
      logger.debug(message)
      continue
    }
    pickedPackages[relDepPath] = pkgSnapshot
    const nextRelDepPaths = R.toPairs(
      {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies && pkgSnapshot.optionalDependencies || {}),
      })
      .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
      .filter((nodeId) => nodeId !== null) as string[]

    pkgAllDeps(pkgSnapshots, pickedPackages, nextRelDepPaths, opts)
  }
}
