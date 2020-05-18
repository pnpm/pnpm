import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  Lockfile,
  PackageSnapshots,
} from '@pnpm/lockfile-types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import pnpmLogger from '@pnpm/logger'
import packageIsInstallable from '@pnpm/package-is-installable'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')
import filterImporter from './filterImporter'
import LockfileMissingDependencyError from './LockfileMissingDependencyError'

const logger = pnpmLogger('lockfile')

export default function filterByImportersAndEngine (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    currentEngine: {
      nodeVersion: string,
      pnpmVersion: string,
    },
    engineStrict: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    includeIncompatiblePackages?: boolean,
    failOnMissingDependencies: boolean,
    lockfileDir: string,
    skipped: Set<string>,
  }
): Lockfile {
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
      currentEngine: opts.currentEngine,
      engineStrict: opts.engineStrict,
      failOnMissingDependencies: opts.failOnMissingDependencies,
      include: opts.include,
      includeIncompatiblePackages: opts.includeIncompatiblePackages === true,
      lockfileDir: opts.lockfileDir,
      skipped: opts.skipped,
    }) || {}

  const importers = importerIds.reduce((acc, importerId) => {
    acc[importerId] = filterImporter(lockfile.importers[importerId], opts.include)
    if (acc[importerId].optionalDependencies) {
      for (const depName of Object.keys(acc[importerId].optionalDependencies || {})) {
        const relDepPath = dp.refToRelative(acc[importerId].optionalDependencies![depName], depName)
        if (relDepPath && !packages[relDepPath]) {
          delete acc[importerId].optionalDependencies![depName]
        }
      }
    }
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
    currentEngine: {
      nodeVersion: string,
      pnpmVersion: string,
    },
    engineStrict: boolean,
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    includeIncompatiblePackages: boolean,
    lockfileDir: string,
    skipped: Set<string>,
  }
) {
  const pickedPackages = {} as PackageSnapshots
  pkgAllDeps({ pkgSnapshots, pickedPackages }, relDepPaths, true, opts)
  return pickedPackages
}

function pkgAllDeps (
  ctx: {
    pkgSnapshots: PackageSnapshots,
    pickedPackages: PackageSnapshots,
  },
  relDepPaths: string[],
  parentIsInstallable: boolean,
  opts: {
    currentEngine: {
      nodeVersion: string,
      pnpmVersion: string,
    },
    engineStrict: boolean,
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    includeIncompatiblePackages: boolean,
    lockfileDir: string,
    skipped: Set<string>,
  }
) {
  for (const relDepPath of relDepPaths) {
    if (ctx.pickedPackages[relDepPath]) continue
    const pkgSnapshot = ctx.pkgSnapshots[relDepPath]
    if (!pkgSnapshot && !relDepPath.startsWith('link:')) {
      if (opts.failOnMissingDependencies) {
        throw new LockfileMissingDependencyError(relDepPath)
      }
      logger.debug(`No entry for "${relDepPath}" in ${WANTED_LOCKFILE}`)
      continue
    }
    let installable!: boolean
    if (!parentIsInstallable) {
      installable = false
      if (!ctx.pickedPackages[relDepPath]) {
        opts.skipped.add(relDepPath)
      }
    } else {
      const pkg = {
        ...nameVerFromPkgSnapshot(relDepPath, pkgSnapshot),
        cpu: pkgSnapshot.cpu,
        engines: pkgSnapshot.engines,
        os: pkgSnapshot.os,
      }
      // TODO: relDepPath is not the package ID. Should be fixed
      installable = opts.includeIncompatiblePackages || packageIsInstallable(pkgSnapshot.id || relDepPath, pkg, {
        engineStrict: opts.engineStrict,
        lockfileDir: opts.lockfileDir,
        nodeVersion: opts.currentEngine.nodeVersion,
        optional: pkgSnapshot.optional === true,
        pnpmVersion: opts.currentEngine.pnpmVersion,
      }) !== false
      if (!installable) {
        if (!ctx.pickedPackages[relDepPath]) {
          opts.skipped.add(relDepPath)
        }
      } else {
        opts.skipped.delete(relDepPath)
        ctx.pickedPackages[relDepPath] = pkgSnapshot
      }
    }
    const nextRelDepPaths = R.toPairs(
      {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies && pkgSnapshot.optionalDependencies || {}),
      })
      .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
      .filter((nodeId) => nodeId !== null) as string[]

    pkgAllDeps(ctx, nextRelDepPaths, installable, opts)
  }
}
