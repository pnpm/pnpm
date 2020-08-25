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
import filterImporter from './filterImporter'
import LockfileMissingDependencyError from './LockfileMissingDependencyError'
import R = require('ramda')

const logger = pnpmLogger('lockfile')

export default function filterByImportersAndEngine (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    currentEngine: {
      nodeVersion: string
      pnpmVersion: string
    }
    engineStrict: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages?: boolean
    failOnMissingDependencies: boolean
    lockfileDir: string
    skipped: Set<string>
  }
): Lockfile {
  const importerDeps = importerIds
    .map((importerId) => lockfile.importers[importerId])
    .map((importer) => ({
      ...(opts.include.dependencies ? importer.dependencies : {}),
      ...(opts.include.devDependencies ? importer.devDependencies : {}),
      ...(opts.include.optionalDependencies ? importer.optionalDependencies : {}),
    }))
    .map(Object.entries)
  const directDepPaths = R.unnest(importerDeps)
    .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]

  const packages = (lockfile.packages &&
    pickPkgsWithAllDeps(lockfile.packages, directDepPaths, {
      currentEngine: opts.currentEngine,
      engineStrict: opts.engineStrict,
      failOnMissingDependencies: opts.failOnMissingDependencies,
      include: opts.include,
      includeIncompatiblePackages: opts.includeIncompatiblePackages === true,
      lockfileDir: opts.lockfileDir,
      skipped: opts.skipped,
    })) ?? {}

  const importers = importerIds.reduce((acc, importerId) => {
    acc[importerId] = filterImporter(lockfile.importers[importerId], opts.include)
    if (acc[importerId].optionalDependencies) {
      for (const depName of Object.keys(acc[importerId].optionalDependencies ?? {})) {
        const depPath = dp.refToRelative(acc[importerId].optionalDependencies![depName], depName)
        if (depPath && !packages[depPath]) {
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
  depPaths: string[],
  opts: {
    currentEngine: {
      nodeVersion: string
      pnpmVersion: string
    }
    engineStrict: boolean
    failOnMissingDependencies: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages: boolean
    lockfileDir: string
    skipped: Set<string>
  }
) {
  const pickedPackages = {} as PackageSnapshots
  pkgAllDeps({ pkgSnapshots, pickedPackages }, depPaths, true, opts)
  return pickedPackages
}

function pkgAllDeps (
  ctx: {
    pkgSnapshots: PackageSnapshots
    pickedPackages: PackageSnapshots
  },
  depPaths: string[],
  parentIsInstallable: boolean,
  opts: {
    currentEngine: {
      nodeVersion: string
      pnpmVersion: string
    }
    engineStrict: boolean
    failOnMissingDependencies: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages: boolean
    lockfileDir: string
    skipped: Set<string>
  }
) {
  for (const depPath of depPaths) {
    if (ctx.pickedPackages[depPath]) continue
    const pkgSnapshot = ctx.pkgSnapshots[depPath]
    if (!pkgSnapshot && !depPath.startsWith('link:')) {
      if (opts.failOnMissingDependencies) {
        throw new LockfileMissingDependencyError(depPath)
      }
      logger.debug(`No entry for "${depPath}" in ${WANTED_LOCKFILE}`)
      continue
    }
    let installable!: boolean
    if (!parentIsInstallable) {
      installable = false
      if (!ctx.pickedPackages[depPath] && pkgSnapshot.optional === true) {
        opts.skipped.add(depPath)
      }
    } else {
      const pkg = {
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
        cpu: pkgSnapshot.cpu,
        engines: pkgSnapshot.engines,
        os: pkgSnapshot.os,
      }
      // TODO: depPath is not the package ID. Should be fixed
      installable = opts.includeIncompatiblePackages || packageIsInstallable(pkgSnapshot.id ?? depPath, pkg, {
        engineStrict: opts.engineStrict,
        lockfileDir: opts.lockfileDir,
        nodeVersion: opts.currentEngine.nodeVersion,
        optional: pkgSnapshot.optional === true,
        pnpmVersion: opts.currentEngine.pnpmVersion,
      }) !== false
      if (!installable) {
        if (!ctx.pickedPackages[depPath] && pkgSnapshot.optional === true) {
          opts.skipped.add(depPath)
        }
      } else {
        opts.skipped.delete(depPath)
      }
    }
    ctx.pickedPackages[depPath] = pkgSnapshot
    const nextRelDepPaths = Object.entries(
      {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
      })
      .map(([pkgName, ref]) => {
        if (pkgSnapshot.peerDependencies?.[pkgName]) return null
        return dp.refToRelative(ref, pkgName)
      })
      .filter((nodeId) => nodeId !== null) as string[]

    pkgAllDeps(ctx, nextRelDepPaths, installable, opts)
  }
}
