import pnpmLogger from '@pnpm/logger'
import packageIsInstallable from '@pnpm/package-is-installable'
import {
  PackageSnapshots,
  Shrinkwrap,
} from '@pnpm/shrinkwrap-types'
import { nameVerFromPkgSnapshot } from '@pnpm/shrinkwrap-utils'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')
import filterImporter from './filterImporter'
import normalizeShrinkwrap from './normalizeShrinkwrap'

const logger = pnpmLogger('shrinkwrap')

export default function filterByImportersAndEngine (
  shr: Shrinkwrap,
  importerIds: string[],
  opts: {
    currentEngine: {
      nodeVersion: string,
      pnpmVersion: string,
    },
    engineStrict: boolean,
    defaultRegistry: string,
    include: { [dependenciesField in DependenciesField]: boolean },
    failOnMissingDependencies: boolean,
    prefix: string,
    skipped: Set<string>,
  },
): Shrinkwrap {
  const importerDeps = importerIds
    .map((importerId) => shr.importers[importerId])
    .map((importer) => ({
      ...(opts.include.dependencies && importer.dependencies || {}),
      ...(opts.include.devDependencies && importer.devDependencies || {}),
      ...(opts.include.optionalDependencies && importer.optionalDependencies || {}),
    }))
    .map(R.toPairs)
  const directDepPaths = R.unnest(importerDeps)
    .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]

  const packages = shr.packages &&
    pickPkgsWithAllDeps(shr.packages, directDepPaths, {
      currentEngine: opts.currentEngine,
      defaultRegistry: opts.defaultRegistry,
      engineStrict: opts.engineStrict,
      failOnMissingDependencies: opts.failOnMissingDependencies,
      include: opts.include,
      prefix: opts.prefix,
      skipped: opts.skipped,
    }) || {}

  const importers = importerIds.reduce((acc, importerId) => {
    acc[importerId] = filterImporter(shr.importers[importerId], opts.include)
    if (acc[importerId].optionalDependencies) {
      for (const depName of Object.keys(acc[importerId].optionalDependencies || {})) {
        const relDepPath = dp.refToRelative(acc[importerId].optionalDependencies![depName], depName)
        if (relDepPath && !packages[relDepPath]) {
          delete acc[importerId].optionalDependencies![depName]
        }
      }
    }
    return acc
  }, { ...shr.importers })

  return normalizeShrinkwrap({
    importers,
    packages,
    registry: shr.registry,
    shrinkwrapVersion: shr.shrinkwrapVersion,
  } as Shrinkwrap)
}

function pickPkgsWithAllDeps (
  pkgSnapshots: PackageSnapshots,
  relDepPaths: string[],
  opts: {
    currentEngine: {
      nodeVersion: string,
      pnpmVersion: string,
    },
    defaultRegistry: string,
    engineStrict: boolean,
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    prefix: string,
    skipped: Set<string>,
  },
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
    defaultRegistry: string,
    engineStrict: boolean,
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
    prefix: string,
    skipped: Set<string>,
  },
) {
  for (const relDepPath of relDepPaths) {
    if (ctx.pickedPackages[relDepPath]) continue
    const pkgSnapshot = ctx.pkgSnapshots[relDepPath]
    if (!pkgSnapshot && !relDepPath.startsWith('link:')) {
      const message = `No entry for "${relDepPath}" in shrinkwrap.yaml`
      if (opts.failOnMissingDependencies) {
        const err = new Error(message)
        err['code'] = 'ERR_PNPM_SHRINKWRAP_MISSING_DEPENDENCY' // tslint:disable-line:no-string-literal
        throw err
      }
      logger.debug(message)
      continue
    }
    let installable!: boolean
    const depPath = dp.resolve(opts.defaultRegistry, relDepPath)
    if (!parentIsInstallable) {
      installable = false
      if (!ctx.pickedPackages[relDepPath]) {
        opts.skipped.add(depPath)
      }
    } else {
      const pkg = {
        ...nameVerFromPkgSnapshot(relDepPath, pkgSnapshot),
        cpu: pkgSnapshot.cpu,
        engines: pkgSnapshot.engines,
        os: pkgSnapshot.os,
      }
      // TODO: relDepPath is not the package ID. Should be fixed
      installable = packageIsInstallable(pkgSnapshot.id || relDepPath, pkg, {
        engineStrict: opts.engineStrict,
        nodeVersion: opts.currentEngine.nodeVersion,
        optional: pkgSnapshot.optional === true,
        pnpmVersion: opts.currentEngine.pnpmVersion,
        prefix: opts.prefix,
      })
      if (!installable) {
        if (!ctx.pickedPackages[relDepPath]) {
          opts.skipped.add(depPath)
        }
      } else {
        opts.skipped.delete(depPath)
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
